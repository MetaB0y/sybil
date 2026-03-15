"""Fetch real NBA play-by-play data from ESPN and convert to arena dataset format.

Uses ESPN's public API (no auth required):
- Scoreboard: games for a given date
- Summary: play-by-play, boxscore, injuries per game

Produces a Dataset JSON file with real scores, real play-by-play moments,
and dense news items suitable for AI trading backtests.
"""

import argparse
import json
import time
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path


# ---------------------------------------------------------------------------
# ESPN API helpers
# ---------------------------------------------------------------------------

HEADERS = {"User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"}
ESPN_BASE = "https://site.api.espn.com/apis/site/v2/sports/basketball/nba"


def _fetch_json(url: str) -> dict:
    req = urllib.request.Request(url, headers=HEADERS)
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read())


def fetch_scoreboard(date_str: str) -> dict:
    """Fetch NBA scoreboard for YYYYMMDD date string."""
    url = f"{ESPN_BASE}/scoreboard?dates={date_str}"
    return _fetch_json(url)


def fetch_summary(game_id: str) -> dict:
    """Fetch full game summary (PBP, boxscore, injuries)."""
    url = f"{ESPN_BASE}/summary?event={game_id}"
    return _fetch_json(url)


# ---------------------------------------------------------------------------
# Data extraction
# ---------------------------------------------------------------------------


def _team_abbrev(name: str) -> str:
    """Short abbreviation for event IDs."""
    abbrevs = {
        "Atlanta Hawks": "atl",
        "Boston Celtics": "bos",
        "Brooklyn Nets": "bkn",
        "Charlotte Hornets": "cha",
        "Chicago Bulls": "chi",
        "Cleveland Cavaliers": "cle",
        "Dallas Mavericks": "dal",
        "Denver Nuggets": "den",
        "Detroit Pistons": "det",
        "Golden State Warriors": "gsw",
        "Houston Rockets": "hou",
        "Indiana Pacers": "ind",
        "LA Clippers": "lac",
        "Los Angeles Clippers": "lac",
        "Los Angeles Lakers": "lal",
        "Memphis Grizzlies": "mem",
        "Miami Heat": "mia",
        "Milwaukee Bucks": "mil",
        "Minnesota Timberwolves": "min",
        "New Orleans Pelicans": "nop",
        "New York Knicks": "nyk",
        "Oklahoma City Thunder": "okc",
        "Orlando Magic": "orl",
        "Philadelphia 76ers": "phi",
        "Phoenix Suns": "phx",
        "Portland Trail Blazers": "por",
        "Sacramento Kings": "sac",
        "San Antonio Spurs": "sas",
        "Toronto Raptors": "tor",
        "Utah Jazz": "uta",
        "Washington Wizards": "was",
    }
    return abbrevs.get(name, name[:3].lower())


def parse_game_info(event: dict) -> dict:
    """Extract basic game info from a scoreboard event."""
    comp = event["competitions"][0]
    competitors = comp["competitors"]
    home = [c for c in competitors if c["homeAway"] == "home"][0]
    away = [c for c in competitors if c["homeAway"] == "away"][0]

    home_name = home["team"]["displayName"]
    away_name = away["team"]["displayName"]
    home_score = int(home["score"])
    away_score = int(away["score"])

    date_str = event["date"]  # ISO format

    # Quarter scores
    home_quarters = [int(ls["value"]) for ls in home.get("linescores", [])]
    away_quarters = [int(ls["value"]) for ls in away.get("linescores", [])]

    return {
        "espn_id": event["id"],
        "home_team": home_name,
        "away_team": away_name,
        "home_score": home_score,
        "away_score": away_score,
        "home_quarters": home_quarters,
        "away_quarters": away_quarters,
        "commence_time": date_str,
        "home_team_id": home["team"]["id"],
        "away_team_id": away["team"]["id"],
    }


def get_starters(summary: dict, home_team_id: str, away_team_id: str) -> dict:
    """Extract starting lineups from boxscore."""
    result = {"home": [], "away": []}
    players_data = summary.get("boxscore", {}).get("players", [])
    for team_data in players_data:
        team_id = team_data["team"]["id"]
        stats = team_data.get("statistics", [{}])
        if not stats:
            continue
        athletes = stats[0].get("athletes", [])
        starters = []
        for ath in athletes:
            if ath.get("starter"):
                starters.append(ath["athlete"]["displayName"])
        if team_id == home_team_id:
            result["home"] = starters
        elif team_id == away_team_id:
            result["away"] = starters
    return result


def get_player_stats(summary: dict) -> dict[str, dict]:
    """Extract final player stats from boxscore.

    Returns {player_name: {points, rebounds, assists, ...}}.
    """
    result = {}
    players_data = summary.get("boxscore", {}).get("players", [])
    for team_data in players_data:
        team_name = team_data["team"]["displayName"]
        stats = team_data.get("statistics", [{}])
        if not stats:
            continue
        keys = stats[0].get("keys", [])
        for ath in stats[0].get("athletes", []):
            if ath.get("didNotPlay"):
                continue
            name = ath["athlete"]["displayName"]
            vals = ath.get("stats", [])
            stat_dict = dict(zip(keys, vals))
            try:
                pts = int(stat_dict.get("points", "0"))
                reb = int(stat_dict.get("rebounds", "0"))
                ast = int(stat_dict.get("assists", "0"))
                stl = int(stat_dict.get("steals", "0"))
                blk = int(stat_dict.get("blocks", "0"))
                tov = int(stat_dict.get("turnovers", "0"))
            except (ValueError, TypeError):
                continue
            result[name] = {
                "team": team_name,
                "points": pts,
                "rebounds": reb,
                "assists": ast,
                "steals": stl,
                "blocks": blk,
                "turnovers": tov,
                "starter": ath.get("starter", False),
            }
    return result


# ---------------------------------------------------------------------------
# News generation from play-by-play
# ---------------------------------------------------------------------------


def _estimate_wallclock(
    game_start: datetime, period: int, clock_str: str, num_periods: int
) -> datetime:
    """Estimate a real-time wallclock from game clock.

    Each quarter is ~12 min game clock but ~35 min real time.
    Overtime is ~5 min game clock but ~15 min real time.
    """
    # Parse clock "MM:SS" or "SS.S"
    if ":" in clock_str:
        parts = clock_str.split(":")
        game_secs_remaining = int(parts[0]) * 60 + float(parts[1])
    else:
        game_secs_remaining = float(clock_str)

    # Regulation quarter: 12 min game, ~35 min real
    # Overtime: 5 min game, ~15 min real
    real_time_per_reg_quarter = timedelta(minutes=35)
    real_time_per_ot = timedelta(minutes=15)
    reg_quarter_secs = 12 * 60
    ot_secs = 5 * 60

    elapsed = timedelta()
    for q in range(1, period):
        if q <= 4:
            elapsed += real_time_per_reg_quarter
        else:
            elapsed += real_time_per_ot

    # Time elapsed within current period
    if period <= 4:
        fraction_done = 1.0 - (game_secs_remaining / reg_quarter_secs)
        elapsed += real_time_per_reg_quarter * fraction_done
    else:
        fraction_done = 1.0 - (game_secs_remaining / ot_secs)
        elapsed += real_time_per_ot * fraction_done

    return game_start + elapsed


def _use_wallclock_or_estimate(
    play: dict, game_start: datetime
) -> datetime:
    """Use real wallclock from play data if available, else estimate."""
    wc = play.get("wallclock")
    if wc:
        try:
            # ESPN wallclocks are like "2025-12-16T00:12:32Z"
            return datetime.fromisoformat(wc.replace("Z", "+00:00"))
        except (ValueError, TypeError):
            pass
    period = play["period"]["number"]
    clock = play["clock"]["displayValue"]
    return _estimate_wallclock(game_start, period, clock, 4)


def generate_news_items(
    game_info: dict,
    summary: dict,
    event_id: str,
) -> list[dict]:
    """Generate news items from game data.

    Returns list of dicts ready to be serialized as NewsItem.
    """
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]
    game_start = datetime.fromisoformat(
        game_info["commence_time"].replace("Z", "+00:00")
    )
    plays = summary.get("plays", [])
    home_team_id = game_info["home_team_id"]
    away_team_id = game_info["away_team_id"]

    # ---- 1. Starting lineups (30 min before tipoff) ----
    starters = get_starters(summary, home_team_id, away_team_id)
    lineup_time = game_start - timedelta(minutes=30)
    home_starters_str = ", ".join(starters["home"]) if starters["home"] else "TBD"
    away_starters_str = ", ".join(starters["away"]) if starters["away"] else "TBD"
    news.append({
        "timestamp": lineup_time.isoformat(),
        "headline": f"Starting lineups announced: {away} at {home}",
        "content": (
            f"{home} start {home_starters_str}. "
            f"{away} go with {away_starters_str}."
        ),
        "source": "lineup",
        "event_id": event_id,
        "metadata": {"home_team": home, "away_team": away},
    })

    # ---- 2. Quarter-end scores ----
    hq = game_info["home_quarters"]
    aq = game_info["away_quarters"]
    num_periods = len(hq)

    for i in range(num_periods):
        period_num = i + 1
        cum_home = sum(hq[: i + 1])
        cum_away = sum(aq[: i + 1])

        if period_num <= 4:
            period_label = f"Q{period_num}"
        else:
            ot_num = period_num - 4
            period_label = f"OT{ot_num}" if ot_num > 1 else "OT"

        # Find the "End Period" play for this period to get wallclock
        end_play = None
        for p in plays:
            if (
                p["type"]["text"] == "End Period"
                and p["period"]["number"] == period_num
            ):
                end_play = p
                break

        if end_play:
            ts = _use_wallclock_or_estimate(end_play, game_start)
        else:
            ts = _estimate_wallclock(game_start, period_num, "0:00", num_periods)

        if period_num == num_periods:
            # Final score
            is_ot = num_periods > 4
            ot_tag = " (OT)" if is_ot else ""
            winner = home if cum_home > cum_away else away
            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"Final{ot_tag}: {home} {cum_home} - {away} {cum_away}",
                "content": _final_summary(game_info, summary),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "final": True,
                    "home_score": cum_home,
                    "away_score": cum_away,
                    "winner": winner,
                },
            })
        else:
            diff = cum_home - cum_away
            if diff > 0:
                lead_str = f"{home} lead by {diff}"
            elif diff < 0:
                lead_str = f"{away} lead by {-diff}"
            else:
                lead_str = "Game is tied"

            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"End of {period_label}: {home} {cum_home} - {away} {cum_away}",
                "content": f"{lead_str} at the end of the {period_label}.",
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "quarter": period_num,
                    "home_score": cum_home,
                    "away_score": cum_away,
                    "home_team": home,
                    "away_team": away,
                },
            })

    # ---- 3. Halftime report ----
    news.extend(_halftime_report(plays, game_info, summary, event_id, game_start))

    # ---- 4. Big quarter detection ----
    news.extend(_detect_big_quarters(game_info, plays, event_id, game_start))

    # ---- 5. Scoring runs (8+ unanswered points) ----
    news.extend(_detect_scoring_runs(plays, game_info, event_id, game_start))

    # ---- 6. Lead changes ----
    news.extend(_detect_lead_changes(plays, game_info, event_id, game_start))

    # ---- 7. Timeouts after runs ----
    news.extend(_detect_timeout_moments(plays, game_info, event_id, game_start))

    # ---- 8. Player milestones ----
    news.extend(_detect_player_milestones(summary, game_info, event_id, game_start))

    # ---- 9. Clutch plays (Q4 last 5 min, close game) ----
    news.extend(_detect_clutch_plays(plays, game_info, event_id, game_start))

    # Sort by timestamp and deduplicate
    news.sort(key=lambda n: n["timestamp"])

    return news


def _halftime_report(
    plays: list[dict],
    game_info: dict,
    summary: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Generate a halftime report with top performers so far."""
    home = game_info["home_team"]
    away = game_info["away_team"]
    hq = game_info["home_quarters"]
    aq = game_info["away_quarters"]

    if len(hq) < 2:
        return []

    half_home = sum(hq[:2])
    half_away = sum(aq[:2])
    diff = half_home - half_away

    # Find halftime play timestamp
    end_q2 = None
    for p in plays:
        if p["type"]["text"] == "End Period" and p["period"]["number"] == 2:
            end_q2 = p
            break

    if end_q2:
        ts = _use_wallclock_or_estimate(end_q2, game_start)
    else:
        ts = _estimate_wallclock(game_start, 2, "0:00", len(hq))

    # Offset by 2 minutes after Q2 end to avoid duplicate timestamp
    ts = ts + timedelta(minutes=2)

    if diff > 0:
        lead_text = f"{home} lead by {diff}"
    elif diff < 0:
        lead_text = f"{away} lead by {-diff}"
    else:
        lead_text = "Game tied"

    # Find the leading scorer at halftime from play-by-play scoring
    # We'll use final stats as a proxy with a note
    player_stats = get_player_stats(summary)
    top_scorers = sorted(player_stats.items(), key=lambda x: x[1]["points"], reverse=True)[:2]
    scorer_text = ""
    if top_scorers:
        parts = []
        for name, stats in top_scorers:
            parts.append(f"{name} ({stats['team']}) {stats['points']} pts")
        scorer_text = f" Top performers: {', '.join(parts)}."

    return [{
        "timestamp": ts.isoformat(),
        "headline": f"Halftime: {home} {half_home} - {away} {half_away}",
        "content": f"{lead_text} at the half.{scorer_text}",
        "source": "in_game",
        "event_id": event_id,
        "metadata": {
            "quarter": 2,
            "home_score": half_home,
            "away_score": half_away,
            "home_team": home,
            "away_team": away,
            "halftime": True,
        },
    }]


def _detect_big_quarters(
    game_info: dict,
    plays: list[dict],
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect quarters where a team scores 35+ points or outscores opponent by 10+."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]
    hq = game_info["home_quarters"]
    aq = game_info["away_quarters"]
    num_periods = len(hq)

    for i in range(min(num_periods, 4)):  # Only regulation quarters
        period_num = i + 1
        h_pts = hq[i]
        a_pts = aq[i]

        # Find end-of-quarter timestamp
        end_play = None
        for p in plays:
            if p["type"]["text"] == "End Period" and p["period"]["number"] == period_num:
                end_play = p
                break

        if end_play:
            ts = _use_wallclock_or_estimate(end_play, game_start)
        else:
            ts = _estimate_wallclock(game_start, period_num, "0:00", num_periods)

        # Offset slightly to avoid duplicate with quarter-end news
        ts = ts + timedelta(minutes=1)

        # Big scoring quarter (35+)
        if h_pts >= 35:
            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"Big quarter: {home} score {h_pts} in Q{period_num}",
                "content": (
                    f"{home} explode for {h_pts} points in the {_ordinal(period_num)} quarter, "
                    f"outscoring {away} {h_pts}-{a_pts}."
                ),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "quarter": period_num,
                    "home_team": home,
                    "away_team": away,
                },
            })
        elif a_pts >= 35:
            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"Big quarter: {away} score {a_pts} in Q{period_num}",
                "content": (
                    f"{away} explode for {a_pts} points in the {_ordinal(period_num)} quarter, "
                    f"outscoring {home} {a_pts}-{h_pts}."
                ),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "quarter": period_num,
                    "home_team": home,
                    "away_team": away,
                },
            })
        # Dominant quarter (10+ margin)
        elif abs(h_pts - a_pts) >= 10:
            dominant = home if h_pts > a_pts else away
            margin = abs(h_pts - a_pts)
            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"Dominant Q{period_num}: {dominant} outscore opponent by {margin}",
                "content": (
                    f"{dominant} win the {_ordinal(period_num)} quarter {max(h_pts, a_pts)}-{min(h_pts, a_pts)}, "
                    f"a {margin}-point advantage."
                ),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "quarter": period_num,
                    "home_team": home,
                    "away_team": away,
                },
            })

    return news


def _ordinal(n: int) -> str:
    """Return ordinal string for a number (1st, 2nd, 3rd, 4th)."""
    if n == 1:
        return "1st"
    elif n == 2:
        return "2nd"
    elif n == 3:
        return "3rd"
    else:
        return f"{n}th"


def _final_summary(game_info: dict, summary: dict) -> str:
    """Generate a one-line final game summary."""
    home = game_info["home_team"]
    away = game_info["away_team"]
    hs = game_info["home_score"]
    as_ = game_info["away_score"]
    winner = home if hs > as_ else away
    loser = away if hs > as_ else home

    # Find top scorer
    player_stats = get_player_stats(summary)
    top_scorer = None
    top_pts = 0
    for name, stats in player_stats.items():
        if stats["points"] > top_pts:
            top_pts = stats["points"]
            top_scorer = name

    parts = [f"{winner} defeat {loser}!"]
    if top_scorer:
        s = player_stats[top_scorer]
        parts.append(
            f"{top_scorer} leads with {s['points']} pts, "
            f"{s['rebounds']} reb, {s['assists']} ast."
        )
    return " ".join(parts)


def _detect_scoring_runs(
    plays: list[dict],
    game_info: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect 8+ unanswered scoring runs."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]
    home_team_id = game_info["home_team_id"]

    prev_home = 0
    prev_away = 0
    run_team = None
    run_points = 0

    scoring_plays = [p for p in plays if p.get("scoringPlay")]

    for play in scoring_plays:
        cur_home = play["homeScore"]
        cur_away = play["awayScore"]
        home_scored = cur_home - prev_home
        away_scored = cur_away - prev_away

        if home_scored > 0 and away_scored == 0:
            scoring_team_id = "home"
        elif away_scored > 0 and home_scored == 0:
            scoring_team_id = "away"
        else:
            # Both scored somehow (rare), reset
            run_team = None
            run_points = 0
            prev_home = cur_home
            prev_away = cur_away
            continue

        if scoring_team_id == run_team:
            run_points += home_scored + away_scored
        else:
            run_team = scoring_team_id
            run_points = home_scored + away_scored

        if run_points >= 8 and run_points - (home_scored + away_scored) < 8:
            # Just crossed the 8-point threshold
            team_name = home if run_team == "home" else away
            ts = _use_wallclock_or_estimate(play, game_start)
            period = play["period"]["number"]
            clock = play["clock"]["displayValue"]
            news.append({
                "timestamp": ts.isoformat(),
                "headline": f"Scoring run: {team_name} on a {run_points}-0 run",
                "content": (
                    f"{team_name} have scored {run_points} unanswered points. "
                    f"Score: {home} {cur_home} - {away} {cur_away} "
                    f"({clock} remaining in period {period})."
                ),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "quarter": period,
                    "home_score": cur_home,
                    "away_score": cur_away,
                    "home_team": home,
                    "away_team": away,
                    "run_team": team_name,
                    "run_points": run_points,
                },
            })

        prev_home = cur_home
        prev_away = cur_away

    return news


def _detect_lead_changes(
    plays: list[dict],
    game_info: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect significant lead changes (where team goes from trailing to leading)."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]

    prev_leader = None
    change_count = 0
    last_reported_change = 0

    scoring_plays = [p for p in plays if p.get("scoringPlay")]

    for play in scoring_plays:
        cur_home = play["homeScore"]
        cur_away = play["awayScore"]

        if cur_home > cur_away:
            leader = "home"
        elif cur_away > cur_home:
            leader = "away"
        else:
            leader = None  # tied

        if prev_leader is not None and leader is not None and leader != prev_leader:
            change_count += 1

            # Report lead changes sparingly: after Q1, and roughly every 3rd change
            if change_count >= last_reported_change + 3:
                last_reported_change = change_count
                team_name = home if leader == "home" else away
                ts = _use_wallclock_or_estimate(play, game_start)
                period = play["period"]["number"]
                clock = play["clock"]["displayValue"]
                news.append({
                    "timestamp": ts.isoformat(),
                    "headline": f"Lead change: {team_name} take the lead",
                    "content": (
                        f"Lead change #{change_count}! {team_name} now lead "
                        f"{cur_home}-{cur_away} with {clock} left in period {period}."
                    ),
                    "source": "in_game",
                    "event_id": event_id,
                    "metadata": {
                        "quarter": period,
                        "home_score": cur_home,
                        "away_score": cur_away,
                        "home_team": home,
                        "away_team": away,
                        "lead_changes": change_count,
                    },
                })

        prev_leader = leader

    return news


def _detect_timeout_moments(
    plays: list[dict],
    game_info: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect timeouts called after the opposing team scores 6+ unanswered."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]

    # Track recent scoring to detect timeouts during runs
    prev_home = 0
    prev_away = 0
    run_team = None
    run_points = 0

    for play in plays:
        if play.get("scoringPlay"):
            cur_home = play["homeScore"]
            cur_away = play["awayScore"]
            home_scored = cur_home - prev_home
            away_scored = cur_away - prev_away

            if home_scored > 0 and away_scored == 0:
                st = "home"
            elif away_scored > 0 and home_scored == 0:
                st = "away"
            else:
                run_team = None
                run_points = 0
                prev_home = cur_home
                prev_away = cur_away
                continue

            if st == run_team:
                run_points += home_scored + away_scored
            else:
                run_team = st
                run_points = home_scored + away_scored

            prev_home = cur_home
            prev_away = cur_away

        if "Timeout" in play["type"]["text"] and run_points >= 6:
            # A timeout was called while opponent is on a run
            # Figure out who called it from play text
            timeout_text = play.get("text", "")
            calling_team = None
            if home.split()[-1].lower() in timeout_text.lower():
                calling_team = home
            elif away.split()[-1].lower() in timeout_text.lower():
                calling_team = away

            running_team = home if run_team == "home" else away
            ts = _use_wallclock_or_estimate(play, game_start)
            period = play["period"]["number"]

            if calling_team and calling_team != running_team:
                news.append({
                    "timestamp": ts.isoformat(),
                    "headline": f"Timeout: {calling_team} try to stop {run_points}-0 run",
                    "content": (
                        f"{calling_team} call timeout to stop {running_team}'s "
                        f"{run_points}-0 scoring run. "
                        f"Score: {home} {play['homeScore']} - {away} {play['awayScore']}."
                    ),
                    "source": "in_game",
                    "event_id": event_id,
                    "metadata": {
                        "quarter": period,
                        "home_score": play["homeScore"],
                        "away_score": play["awayScore"],
                        "home_team": home,
                        "away_team": away,
                    },
                })

    return news


def _detect_player_milestones(
    summary: dict,
    game_info: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect player stat milestones: 25+ pts, 10+ reb, near triple-double."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]
    num_periods = len(game_info["home_quarters"])

    # Use final stats to generate milestone news at ~75% through the game
    milestone_time = _estimate_wallclock(game_start, min(4, num_periods), "3:00", num_periods)
    player_stats = get_player_stats(summary)

    for name, stats in player_stats.items():
        pts = stats["points"]
        reb = stats["rebounds"]
        ast = stats["assists"]
        stl = stats["steals"]
        blk = stats["blocks"]
        team = stats["team"]

        # 30+ points
        if pts >= 30:
            news.append({
                "timestamp": milestone_time.isoformat(),
                "headline": f"Player milestone: {name} with {pts} points",
                "content": (
                    f"{name} ({team}) is having a big night with {pts} points, "
                    f"{reb} rebounds, and {ast} assists."
                ),
                "source": "in_game",
                "event_id": event_id,
                "metadata": {
                    "player": name,
                    "team": team,
                    "points": pts,
                    "rebounds": reb,
                    "assists": ast,
                    "home_team": home,
                    "away_team": away,
                },
            })
        # Triple-double or near
        categories_10 = sum(1 for x in [pts, reb, ast, stl, blk] if x >= 10)
        if categories_10 >= 2 and pts >= 20:
            close_cats = [
                c
                for c, v in [
                    ("points", pts),
                    ("rebounds", reb),
                    ("assists", ast),
                    ("steals", stl),
                    ("blocks", blk),
                ]
                if 7 <= v < 10
            ]
            if categories_10 >= 3:
                label = "Triple-double"
            elif close_cats:
                label = "Triple-double watch"
            else:
                label = "Double-double"

            # Only report triple-doubles and triple-double watches (double-doubles are common)
            if label != "Double-double":
                td_time = milestone_time + timedelta(minutes=2)
                news.append({
                    "timestamp": td_time.isoformat(),
                    "headline": f"{label}: {name} ({pts}/{reb}/{ast})",
                    "content": (
                        f"{name} ({team}) {'has a' if categories_10 >= 3 else 'is approaching a'} "
                        f"triple-double with {pts} points, {reb} rebounds, "
                        f"and {ast} assists."
                    ),
                    "source": "in_game",
                    "event_id": event_id,
                    "metadata": {
                        "player": name,
                        "team": team,
                        "points": pts,
                        "rebounds": reb,
                        "assists": ast,
                        "home_team": home,
                        "away_team": away,
                    },
                })

    return news


def _detect_clutch_plays(
    plays: list[dict],
    game_info: dict,
    event_id: str,
    game_start: datetime,
) -> list[dict]:
    """Detect clutch scoring plays in Q4/OT last 3 minutes when margin <= 5."""
    news = []
    home = game_info["home_team"]
    away = game_info["away_team"]
    num_periods = len(game_info["home_quarters"])

    for play in plays:
        if not play.get("scoringPlay"):
            continue

        period = play["period"]["number"]
        clock_str = play["clock"]["displayValue"]

        # Only care about Q4 or overtime
        if period < 4:
            continue

        # Parse clock
        if ":" in clock_str:
            parts = clock_str.split(":")
            secs_left = int(parts[0]) * 60 + float(parts[1])
        else:
            secs_left = float(clock_str)

        # Last 3 minutes of Q4, or all of OT
        if period == 4 and secs_left > 180:
            continue

        cur_home = play["homeScore"]
        cur_away = play["awayScore"]
        margin = abs(cur_home - cur_away)

        if margin > 5:
            continue

        # This is a clutch play
        score_val = play.get("scoreValue", 0)
        if score_val < 2:
            continue  # Skip free throws for cleaner news

        play_text = play.get("text", "")
        ts = _use_wallclock_or_estimate(play, game_start)

        # Determine which team scored
        team_data = play.get("team", {})
        scoring_team_id = team_data.get("id", "")

        news.append({
            "timestamp": ts.isoformat(),
            "headline": f"Clutch: {play_text}",
            "content": (
                f"Clutch play with {clock_str} left in period {period}! "
                f"Score: {home} {cur_home} - {away} {cur_away}."
            ),
            "source": "in_game",
            "event_id": event_id,
            "metadata": {
                "quarter": period,
                "home_score": cur_home,
                "away_score": cur_away,
                "home_team": home,
                "away_team": away,
                "clutch": True,
            },
        })

    # Limit clutch plays to avoid flooding — keep at most 5 per game
    if len(news) > 5:
        # Keep the ones closest to the end (latest timestamp)
        news.sort(key=lambda n: n["timestamp"], reverse=True)
        news = news[:5]

    return news


# ---------------------------------------------------------------------------
# Main: build dataset
# ---------------------------------------------------------------------------


def build_dataset(date: str) -> dict:
    """Build a complete dataset for a given date.

    Args:
        date: YYYY-MM-DD format

    Returns:
        dict matching the Dataset schema
    """
    date_obj = datetime.strptime(date, "%Y-%m-%d")
    date_yyyymmdd = date_obj.strftime("%Y%m%d")
    date_mmdd = date_obj.strftime("%m%d")

    print(f"Fetching NBA scoreboard for {date}...")
    scoreboard = fetch_scoreboard(date_yyyymmdd)
    raw_events = scoreboard.get("events", [])
    print(f"Found {len(raw_events)} games.")

    if not raw_events:
        print("No games found for this date.")
        return None

    events = []
    all_news = []
    earliest_time = None
    latest_time = None

    for i, raw_event in enumerate(raw_events):
        game_info = parse_game_info(raw_event)
        espn_id = game_info["espn_id"]

        print(
            f"\n[{i+1}/{len(raw_events)}] {game_info['away_team']} @ {game_info['home_team']} "
            f"(ESPN ID: {espn_id})"
        )

        # Create event ID
        away_abbr = _team_abbrev(game_info["away_team"])
        home_abbr = _team_abbrev(game_info["home_team"])
        event_id = f"nba_{away_abbr}_{home_abbr}_{date_mmdd}"

        # Fetch detailed summary
        time.sleep(0.6)  # Rate limit
        print(f"  Fetching play-by-play...")
        try:
            summary = fetch_summary(espn_id)
        except Exception as e:
            print(f"  WARNING: Could not fetch summary: {e}")
            summary = {}

        plays = summary.get("plays", [])
        print(f"  Got {len(plays)} plays.")

        # Determine outcome
        hs = game_info["home_score"]
        as_ = game_info["away_score"]
        if hs > as_:
            outcome = "home"
        elif as_ > hs:
            outcome = "away"
        else:
            outcome = "draw"

        # Parse commence time
        commence = datetime.fromisoformat(
            game_info["commence_time"].replace("Z", "+00:00")
        )
        # Estimate end time: ~2.5h for regulation, +15min per OT
        num_periods = len(game_info["home_quarters"])
        ot_periods = max(0, num_periods - 4)
        game_duration = timedelta(hours=2, minutes=30) + timedelta(
            minutes=15 * ot_periods
        )
        end_time = commence + game_duration

        if earliest_time is None or commence < earliest_time:
            earliest_time = commence
        if latest_time is None or end_time > latest_time:
            latest_time = end_time

        # Build event
        event = {
            "event_id": event_id,
            "home_team": game_info["home_team"],
            "away_team": game_info["away_team"],
            "commence_time": commence.isoformat(),
            "end_time": end_time.isoformat(),
            "actual_outcome": outcome,
            "final_score": {"home": hs, "away": as_},
            "markets": [
                {
                    "market_name": f"{game_info['home_team']} beats {game_info['away_team']}",
                    "market_type": "moneyline",
                }
            ],
        }
        events.append(event)

        # Generate news
        game_news = generate_news_items(game_info, summary, event_id)
        all_news.extend(game_news)
        print(f"  Generated {len(game_news)} news items.")

    # Lineup news should come before game start
    # Extend time range to include pre-game news
    if earliest_time:
        earliest_time -= timedelta(minutes=30)

    # Sort all news by timestamp
    all_news.sort(key=lambda n: n["timestamp"])

    # Build the final dataset
    nice_date = date_obj.strftime("%B %d, %Y")
    dataset = {
        "name": f"NBA Game Night - {nice_date}",
        "sport": "basketball_nba",
        "time_range": [
            earliest_time.isoformat() if earliest_time else "",
            latest_time.isoformat() if latest_time else "",
        ],
        "events": events,
        "news": all_news,
    }

    return dataset


def main():
    parser = argparse.ArgumentParser(
        description="Fetch real NBA data and convert to arena dataset format."
    )
    parser.add_argument(
        "--date",
        default="2025-12-15",
        help="Game date in YYYY-MM-DD format (default: 2025-12-15)",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Output file path (default: auto-generated in datasets/)",
    )
    args = parser.parse_args()

    dataset = build_dataset(args.date)
    if dataset is None:
        return

    # Determine output path
    if args.output:
        output_path = Path(args.output)
    else:
        date_slug = args.date.replace("-", "")
        output_path = Path(__file__).parent.parent / "datasets" / f"nba_{date_slug}.json"

    output_path.parent.mkdir(parents=True, exist_ok=True)

    with open(output_path, "w") as f:
        json.dump(dataset, f, indent=2)

    # Print summary
    num_events = len(dataset["events"])
    num_news = len(dataset["news"])
    print(f"\nDataset saved to: {output_path}")
    print(f"  Events: {num_events}")
    print(f"  News items: {num_news}")
    print(f"  Time range: {dataset['time_range'][0]} to {dataset['time_range'][1]}")

    # Validate it loads correctly
    print("\nValidating dataset loads correctly...")
    import sys
    sys.path.insert(0, str(Path(__file__).parent.parent))
    from backtest.dataset import Dataset
    ds = Dataset.load(output_path)
    print(f"  Loaded: {ds.name}")
    print(f"  Events: {len(ds.events)}")
    print(f"  News: {len(ds.news)}")
    for ev in ds.events:
        ev_news = ds.get_news_for_event(ev.event_id)
        print(f"    {ev.event_id}: {ev.away_team} @ {ev.home_team} -> {ev.actual_outcome} "
              f"({ev.final_score.home}-{ev.final_score.away}), {len(ev_news)} news items")


if __name__ == "__main__":
    main()
