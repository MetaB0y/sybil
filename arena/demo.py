"""Arena Demo: AI Sports Trading Tournament (Backtest).

One-command demo: paste API keys into .env, run `just arena-demo`,
watch Claude vs GPT compete as sports bettors processing live NBA game news.

Usage:
    uv run python demo.py [--no-server] [--compression 120] [--dataset PATH]
"""

import argparse
import asyncio
import os
import signal
import subprocess
import sys
from pathlib import Path

from rich.console import Console

from backtest.agent import BacktestAgentConfig
from backtest.dataset import Dataset
from backtest.runner import BacktestRunner

console = Console()

DATASET_DIR = Path(__file__).parent / "datasets"
DEFAULT_DATASET = DATASET_DIR / "nba_20251215.json"
FALLBACK_DATASET = DATASET_DIR / "nba_sample.json"


def load_dotenv(path: Path | None = None) -> None:
    """Load .env file into os.environ (simple built-in parser, no deps)."""
    env_path = path or Path(__file__).parent / ".env"
    if not env_path.exists():
        return
    with open(env_path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" not in line:
                continue
            key, _, value = line.partition("=")
            key = key.strip()
            value = value.strip()
            # Strip surrounding quotes
            if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
                value = value[1:-1]
            os.environ.setdefault(key, value)


def _score_watcher(markets: dict[str, "MarketView"]) -> dict[str, float]:
    """Estimate win probability from score margin, quarter, injuries, and momentum.

    Uses a logistic model based on point margin scaled by game progress.
    """
    import math
    import re

    estimates = {}
    for name, view in markets.items():
        quarter = 0
        home_score = 0
        away_score = 0
        has_score = False
        injury_penalty = 0.0  # negative = hurts home, positive = hurts away
        momentum = 0  # positive = home momentum

        for news_line in view.news:
            # Match FINAL
            m = re.search(r"\[FINAL\] (\d+)-(\d+)", news_line)
            if m:
                home_score, away_score = int(m.group(1)), int(m.group(2))
                estimates[name] = 0.95 if home_score > away_score else 0.05
                has_score = True
                break

            # Match score lines: [Q{n} {home}-{away}] or [Q{n} END] {home}-{away}
            if not has_score:
                m = re.search(r"\[Q(\d)\s+(?:END\])?\s*(\d+)-(\d+)", news_line)
                if not m:
                    m = re.search(r"\[Q(\d) END\] (\d+) - (\d+)", news_line)
                if m:
                    quarter = int(m.group(1))
                    home_score = int(m.group(2))
                    away_score = int(m.group(3))
                    has_score = True

            # Track injuries
            if "[INJURY]" in news_line:
                # Rough heuristic: each injury shifts prob ~3%
                # We don't know which team, but metadata has home/away info
                # For now, just note we saw injuries
                injury_penalty += 0.03

            # Track momentum (runs)
            if "run" in news_line.lower():
                # Home team run = positive momentum
                if any(
                    t in news_line for t in name.split(" vs ")[0:1]
                ):
                    momentum += 1
                else:
                    momentum -= 1

        if has_score and name in estimates:
            # FINAL already handled
            continue

        if has_score and quarter > 0:
            margin = home_score - away_score

            # Game progress: 0.0 (start) to 1.0 (end of Q4)
            progress = min(1.0, quarter / 4.0)

            # Logistic model: margin matters more as game progresses.
            # A 10-point lead in Q4 ≈ 85% win prob.
            # Scale factor increases with progress.
            k = 0.08 + 0.12 * progress  # steepness: 0.08 early, 0.20 late
            logit = k * margin
            prob = 1.0 / (1.0 + math.exp(-logit))

            # Small momentum adjustment (±2%)
            prob += momentum * 0.02

            estimates[name] = max(0.02, min(0.98, prob))

    return estimates


def build_agent_configs() -> list[BacktestAgentConfig]:
    """Build bot lineup based on available API keys."""
    from bots.backtest_mm import BacktestTightMM, BacktestWideMM
    from bots.strategy_agent import StrategyAgent

    configs: list[BacktestAgentConfig] = []

    # Market makers for liquidity
    configs.append(BacktestAgentConfig(BacktestTightMM, "MM-Tight", {}))
    configs.append(BacktestAgentConfig(BacktestWideMM, "MM-Wide", {}))

    # Simple rule-based baseline
    configs.append(
        BacktestAgentConfig(
            StrategyAgent,
            "ScoreBot",
            {"strategy_fn": _score_watcher, "edge_threshold": 0.03},
        )
    )

    # Add LLM bots based on available keys
    anthropic_key = os.environ.get("ANTHROPIC_API_KEY", "").strip()
    openai_key = os.environ.get("OPENAI_API_KEY", "").strip()

    if anthropic_key:
        from bots.llm_news_trader import CONTRARIAN_SYSTEM_PROMPT, LLMNewsTrader

        configs.append(
            BacktestAgentConfig(
                LLMNewsTrader,
                "Claude-Sonnet",
                {
                    "provider": "anthropic",
                    "model_name": "claude-sonnet-4-5-20250929",
                    "api_key": anthropic_key,
                },
            )
        )
        configs.append(
            BacktestAgentConfig(
                LLMNewsTrader,
                "Claude-Haiku",
                {
                    "provider": "anthropic",
                    "model_name": "claude-haiku-4-5-20251001",
                    "api_key": anthropic_key,
                    "system_prompt": CONTRARIAN_SYSTEM_PROMPT,
                },
            )
        )
        console.print("[green]Anthropic API key found - adding Claude bots[/green]")
    else:
        console.print("[yellow]No ANTHROPIC_API_KEY - skipping Claude bots[/yellow]")

    if openai_key:
        from bots.llm_news_trader import LLMNewsTrader

        configs.append(
            BacktestAgentConfig(
                LLMNewsTrader,
                "GPT",
                {
                    "provider": "openai",
                    "model_name": "gpt-4o",
                    "api_key": openai_key,
                },
            )
        )
        console.print("[green]OpenAI API key found - adding GPT bot[/green]")
    else:
        console.print("[yellow]No OPENAI_API_KEY - skipping GPT bot[/yellow]")

    if not anthropic_key and not openai_key:
        console.print(
            "[bold yellow]No LLM API keys found. Running with rule-based bots only.[/bold yellow]"
        )
        console.print("Set ANTHROPIC_API_KEY and/or OPENAI_API_KEY in .env for AI bots.\n")

    return configs


async def wait_for_server(base_url: str, timeout: float = 30.0) -> bool:
    """Wait for sybil-api to become healthy."""
    import httpx

    deadline = asyncio.get_event_loop().time() + timeout
    while asyncio.get_event_loop().time() < deadline:
        try:
            async with httpx.AsyncClient() as client:
                resp = await client.get(f"{base_url}/v1/health", timeout=2.0)
                if resp.status_code == 200:
                    return True
        except Exception:
            pass
        await asyncio.sleep(0.5)
    return False


def _kill_existing_server(port: int) -> None:
    """Kill any existing sybil-api process on the given port."""
    try:
        result = subprocess.run(
            ["lsof", "-ti", f":{port}"],
            capture_output=True, text=True, timeout=5,
        )
        pids = result.stdout.strip().split()
        for pid in pids:
            if pid:
                console.print(f"[yellow]Killing existing process on port {port} (pid {pid})[/yellow]")
                os.kill(int(pid), signal.SIGTERM)
        if pids and pids[0]:
            import time
            time.sleep(1)
    except Exception:
        pass


def start_server(port: int = 3001) -> subprocess.Popen:
    """Start sybil-api as a subprocess, killing any existing server first."""
    _kill_existing_server(port)

    project_root = Path(__file__).parent.parent
    cmd = [
        "cargo",
        "run",
        "--release",
        "-p",
        "sybil-api",
        "--",
        "--dev-mode",
        "--port",
        str(port),
    ]
    console.print(f"[bold]Building & starting sybil-api on port {port}...[/bold]")
    proc = subprocess.Popen(
        cmd,
        cwd=project_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return proc


async def run_demo(
    base_url: str,
    dataset_path: Path,
    compression: float,
    initial_balance: float = 200.0,
) -> None:
    """Run the backtest demo."""
    # Load dataset
    if dataset_path.exists():
        dataset = Dataset.load(dataset_path)
    elif FALLBACK_DATASET.exists():
        console.print(f"[yellow]Dataset {dataset_path} not found, using fallback[/yellow]")
        dataset = Dataset.load(FALLBACK_DATASET)
    else:
        console.print("[red]No dataset found![/red]")
        sys.exit(1)

    console.print(f"\n[bold]Loaded dataset: {dataset.name}[/bold]")
    console.print(f"Events: {len(dataset.events)}, News items: {len(dataset.news)}")
    console.print(f"Duration: {dataset.duration / 3600:.1f} hours\n")

    # Build bot lineup
    agent_configs = build_agent_configs()
    console.print(f"\n[bold]Bot lineup ({len(agent_configs)} agents):[/bold]")
    for config in agent_configs:
        console.print(f"  {config.name} ({config.agent_class.__name__})")
    console.print()

    # Run backtest
    runner = BacktestRunner(
        base_url=base_url,
        dataset=dataset,
        agent_configs=agent_configs,
        initial_balance=initial_balance,
        compression_ratio=compression,
    )
    await runner.run()


def main() -> None:
    parser = argparse.ArgumentParser(description="Arena Demo: AI Sports Trading Tournament")
    parser.add_argument(
        "--no-server",
        action="store_true",
        help="Don't start sybil-api (use existing server)",
    )
    parser.add_argument(
        "--compression",
        type=float,
        default=120.0,
        help="Time compression ratio (default: 120x)",
    )
    parser.add_argument(
        "--dataset",
        type=Path,
        default=DEFAULT_DATASET,
        help="Path to dataset JSON file",
    )
    parser.add_argument(
        "--balance",
        type=float,
        default=200.0,
        help="Initial balance per agent in dollars (default: 200)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=3001,
        help="Sybil API port (default: 3001)",
    )
    args = parser.parse_args()

    # Load .env
    load_dotenv()

    base_url = os.environ.get("SYBIL_API_URL", f"http://localhost:{args.port}")

    server_proc = None
    if not args.no_server:
        server_proc = start_server(args.port)

    try:
        # Wait for server
        console.print(f"[bold]Waiting for sybil-api at {base_url}...[/bold]")
        if not asyncio.run(wait_for_server(base_url)):
            console.print("[red]Server failed to start! Is sybil-api running?[/red]")
            if args.no_server:
                console.print("Try running without --no-server (default builds & starts server)")
                console.print("Or:  cargo run --release -p sybil-api -- --dev-mode --port 3001")
            sys.exit(1)
        console.print("[green]Server is ready![/green]\n")

        # Run
        asyncio.run(run_demo(base_url, args.dataset, args.compression, args.balance))

    except KeyboardInterrupt:
        console.print("\n[yellow]Interrupted.[/yellow]")
    finally:
        if server_proc:
            console.print("[bold]Stopping sybil-api...[/bold]")
            server_proc.send_signal(signal.SIGTERM)
            try:
                server_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server_proc.kill()


if __name__ == "__main__":
    main()
