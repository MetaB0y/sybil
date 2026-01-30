"""Orderbook Phase Analysis: Compare liquidity across pipeline phases."""

import streamlit as st
import plotly.graph_objects as go

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


def create_depth_chart(
    book: dict,
    clearing_price: float | None = None,
    title: str = "Depth Chart",
    height: int = 300,
) -> go.Figure:
    """Create a depth chart for an orderbook."""
    fig = go.Figure()

    bids = book.get("bids", [])
    asks = book.get("asks", [])

    # Process bids
    if bids:
        sorted_bids = sorted(bids, key=lambda x: x["price"], reverse=True)
        cumulative = 0
        bid_prices = []
        bid_cumulative = []
        for b in sorted_bids:
            cumulative += b["qty"]
            bid_prices.append(b["price"] * 100)
            bid_cumulative.append(cumulative)
        bid_prices = bid_prices[::-1]
        bid_cumulative = bid_cumulative[::-1]

        fig.add_trace(
            go.Scatter(
                x=bid_prices,
                y=bid_cumulative,
                fill="tozeroy",
                fillcolor="rgba(34, 197, 94, 0.4)",
                line=dict(color="rgb(34, 197, 94)", width=2),
                name="Bids",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative: %{y:,}<extra></extra>",
            )
        )

    # Process asks
    if asks:
        sorted_asks = sorted(asks, key=lambda x: x["price"])
        cumulative = 0
        ask_prices = []
        ask_cumulative = []
        for a in sorted_asks:
            cumulative += a["qty"]
            ask_prices.append(a["price"] * 100)
            ask_cumulative.append(cumulative)

        fig.add_trace(
            go.Scatter(
                x=ask_prices,
                y=ask_cumulative,
                fill="tozeroy",
                fillcolor="rgba(239, 68, 68, 0.4)",
                line=dict(color="rgb(239, 68, 68)", width=2),
                name="Asks",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative: %{y:,}<extra></extra>",
            )
        )

    if clearing_price is not None:
        fig.add_vline(
            x=clearing_price * 100,
            line_dash="dash",
            line_color="rgb(168, 85, 247)",
            line_width=2,
            annotation_text=f"Clear: {clearing_price * 100:.1f}%",
            annotation_position="top",
        )

    fig.update_layout(
        title=dict(text=title, font=dict(size=14)),
        xaxis_title="Price (%)",
        yaxis_title="Cumulative Qty",
        hovermode="x unified",
        showlegend=True,
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        height=height,
        margin=dict(l=50, r=20, t=60, b=50),
        xaxis=dict(range=[0, 100]),
    )

    return fig


def create_comparison_chart(
    initial_book: dict | None,
    final_book: dict | None,
    title: str,
    height: int = 300,
) -> go.Figure:
    """Create a comparison depth chart showing initial vs final liquidity."""
    fig = go.Figure()

    # Initial asks (lighter, dashed)
    if initial_book:
        asks = initial_book.get("asks", [])
        if asks:
            sorted_asks = sorted(asks, key=lambda x: x["price"])
            cumulative = 0
            ask_prices = []
            ask_cumulative = []
            for a in sorted_asks:
                cumulative += a["qty"]
                ask_prices.append(a["price"] * 100)
                ask_cumulative.append(cumulative)

            fig.add_trace(
                go.Scatter(
                    x=ask_prices,
                    y=ask_cumulative,
                    fill="tozeroy",
                    fillcolor="rgba(239, 68, 68, 0.15)",
                    line=dict(color="rgba(239, 68, 68, 0.5)", width=1, dash="dash"),
                    name="Initial Asks",
                )
            )

    # Final asks (solid)
    if final_book:
        asks = final_book.get("asks", [])
        if asks:
            sorted_asks = sorted(asks, key=lambda x: x["price"])
            cumulative = 0
            ask_prices = []
            ask_cumulative = []
            for a in sorted_asks:
                cumulative += a["qty"]
                ask_prices.append(a["price"] * 100)
                ask_cumulative.append(cumulative)

            fig.add_trace(
                go.Scatter(
                    x=ask_prices,
                    y=ask_cumulative,
                    fill="tozeroy",
                    fillcolor="rgba(239, 68, 68, 0.4)",
                    line=dict(color="rgb(239, 68, 68)", width=2),
                    name="Final Asks",
                )
            )

    fig.update_layout(
        title=dict(text=title, font=dict(size=14)),
        xaxis_title="Price (%)",
        yaxis_title="Cumulative Quantity",
        hovermode="x unified",
        showlegend=True,
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        height=height,
        margin=dict(l=50, r=20, t=60, b=50),
        xaxis=dict(range=[0, 100]),
    )

    return fig


def main():
    st.set_page_config(page_title="Phase Analysis", page_icon=":books:", layout="wide")
    st.title("Orderbook Phase Analysis")
    st.caption(
        "Compare liquidity consumption across pipeline phases. "
        "For basic depth charts, see the Markets page."
    )

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    # Check for phase snapshots
    has_phases = len(data.phase_snapshots) > 0
    has_initial = data.initial_liquidity is not None
    has_final = data.final_liquidity is not None

    if not has_phases and not has_initial and not has_final:
        st.info(
            "No liquidity data available. Run with `--features viz` to capture snapshots:\n\n"
            "```bash\n"
            "cargo run --bin matching-sim --release --features viz -- --preset small --export-json snapshot.json\n"
            "```"
        )
        return

    # Get markets
    liquidity_source = data.initial_liquidity or data.final_liquidity
    if liquidity_source:
        market_names = data.get_book_markets(liquidity_source)
    elif has_phases:
        # Try to get markets from first phase with liquidity
        market_names = []
        for snap in data.phase_snapshots:
            liq = snap.get("liquidity")
            if liq:
                market_names = data.get_book_markets(liq)
                break

    if not market_names:
        st.warning("No markets found in liquidity data.")
        return

    selected_market = st.selectbox("Select Market", market_names)
    if not selected_market:
        return

    st.divider()

    # Phase slider analysis
    if has_phases:
        st.header("Phase Progression")

        phase_names = []
        for i, snap in enumerate(data.phase_snapshots):
            phase = snap.get("phase", "Unknown")
            if isinstance(phase, dict):
                phase = list(phase.keys())[0] if phase else "Unknown"
            phase_names.append(f"{i}: {phase}")

        selected_phase_idx = st.select_slider(
            "Pipeline Phase",
            options=list(range(len(data.phase_snapshots))),
            format_func=lambda x: phase_names[x],
            help="Slide through phases to see how liquidity is consumed",
        )

        phase_snap = data.phase_snapshots[selected_phase_idx]

        # Phase metrics
        col1, col2, col3, col4 = st.columns(4)
        with col1:
            st.metric("Fills", f"{phase_snap.get('fills_count', 0):,}")
        with col2:
            welfare_dollars = phase_snap.get("welfare", 0) / 1e9
            st.metric("Welfare", f"${welfare_dollars:.2f}")
        with col3:
            phase_fills = phase_snap.get("phase_fills")
            if phase_fills is not None:
                st.metric("Phase Fills", f"+{phase_fills}")
        with col4:
            st.metric("Elapsed", f"{phase_snap.get('elapsed_secs', 0):.3f}s")

        # Get phase liquidity
        phase_liquidity = data.get_phase_liquidity(selected_phase_idx)

        if phase_liquidity:
            books = data.get_books_for_market(selected_market, phase_liquidity)
            if books:
                cols = st.columns(len(books))
                for i, book in enumerate(sorted(books, key=lambda x: x.get("outcome", 0))):
                    outcome = book.get("outcome", 0)
                    outcome_name = "YES" if outcome == 0 else "NO"
                    clearing_price = data.get_clearing_price(selected_market, outcome=outcome)

                    with cols[i]:
                        fig = create_depth_chart(
                            book,
                            clearing_price=clearing_price,
                            title=f"{outcome_name} @ Phase {selected_phase_idx}",
                            height=280,
                        )
                        st.plotly_chart(fig, use_container_width=True)

                        # Stats
                        total_ask = book.get("total_ask_qty", 0)
                        total_bid = book.get("total_bid_qty", 0)
                        st.caption(f"Ask Qty: {total_ask:,} | Bid Qty: {total_bid:,}")
        else:
            st.info("No liquidity snapshot for this phase.")

    st.divider()

    # Initial vs Final comparison
    if has_initial and has_final:
        st.header("Initial → Final Comparison")

        initial_books = data.get_books_for_market(selected_market, data.initial_liquidity)
        final_books = data.get_books_for_market(selected_market, data.final_liquidity)

        if initial_books or final_books:
            outcomes = set()
            for b in initial_books + final_books:
                outcomes.add(b.get("outcome", 0))

            cols = st.columns(len(outcomes))
            for i, outcome in enumerate(sorted(outcomes)):
                outcome_name = "YES" if outcome == 0 else "NO"
                initial_book = next(
                    (b for b in initial_books if b.get("outcome") == outcome), None
                )
                final_book = next(
                    (b for b in final_books if b.get("outcome") == outcome), None
                )

                with cols[i]:
                    fig = create_comparison_chart(
                        initial_book, final_book, f"{outcome_name}: Initial vs Final"
                    )
                    st.plotly_chart(fig, use_container_width=True)

                    # Consumption metric
                    initial_qty = initial_book.get("total_ask_qty", 0) if initial_book else 0
                    final_qty = final_book.get("total_ask_qty", 0) if final_book else 0
                    consumed = initial_qty - final_qty

                    st.metric(
                        "Liquidity Consumed",
                        f"{consumed:,}",
                        delta=f"-{consumed:,}" if consumed > 0 else "0",
                        delta_color="inverse",
                    )

    # Summary table
    st.divider()
    st.header("All Markets Summary")

    liquidity = data.initial_liquidity or data.final_liquidity
    if liquidity:
        summary_data = []
        for book in liquidity.get("books", []):
            outcome_name = "YES" if book.get("outcome", 0) == 0 else "NO"
            summary_data.append(
                {
                    "Market": book.get("market_name", ""),
                    "Outcome": outcome_name,
                    "Best Bid (%)": f"{book.get('best_bid', 0) * 100:.1f}"
                    if book.get("best_bid")
                    else "-",
                    "Best Ask (%)": f"{book.get('best_ask', 0) * 100:.1f}"
                    if book.get("best_ask")
                    else "-",
                    "Spread (%)": f"{book.get('spread', 0) * 100:.2f}"
                    if book.get("spread")
                    else "-",
                    "Ask Qty": book.get("total_ask_qty", 0),
                    "Bid Qty": book.get("total_bid_qty", 0),
                }
            )

        if summary_data:
            import pandas as pd

            df = pd.DataFrame(summary_data)
            st.dataframe(df, use_container_width=True, hide_index=True)


if __name__ == "__main__":
    main()
else:
    main()
