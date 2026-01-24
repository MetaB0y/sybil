"""Orderbook page: Depth charts and liquidity analysis."""

import streamlit as st
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


def create_depth_chart(book: dict, title: str = "Depth Chart") -> go.Figure:
    """Create a depth chart for an orderbook."""
    fig = go.Figure()

    # Bids (green, filled) - sorted by price descending for depth chart
    bids = book.get("bids", [])
    if bids:
        # Sort bids by price descending and calculate cumulative
        sorted_bids = sorted(bids, key=lambda x: x["price"], reverse=True)
        cumulative = 0
        bid_prices = []
        bid_cumulative = []
        for b in sorted_bids:
            cumulative += b["qty"]
            bid_prices.append(b["price"] * 100)
            bid_cumulative.append(cumulative)

        fig.add_trace(
            go.Scatter(
                x=bid_prices,
                y=bid_cumulative,
                fill="tozeroy",
                fillcolor="rgba(0, 128, 0, 0.3)",
                line=dict(color="green", width=2),
                name="Bids",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative Qty: %{y:,}<extra></extra>",
            )
        )

    # Asks (red, filled) - sorted by price ascending for depth chart
    asks = book.get("asks", [])
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
                fillcolor="rgba(255, 0, 0, 0.3)",
                line=dict(color="red", width=2),
                name="Asks",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative Qty: %{y:,}<extra></extra>",
            )
        )

    # Mid price line
    mid_price = book.get("mid_price")
    if mid_price:
        fig.add_vline(
            x=mid_price * 100,
            line_dash="dash",
            line_color="gray",
            annotation_text=f"Mid: {mid_price * 100:.1f}%",
            annotation_position="top",
        )

    fig.update_layout(
        title=title,
        xaxis_title="Price (%)",
        yaxis_title="Cumulative Quantity",
        hovermode="x unified",
        showlegend=True,
        legend=dict(yanchor="top", y=0.99, xanchor="left", x=0.01),
    )

    return fig


def create_exchange_orderbook(
    book: dict, clearing_price: float | None = None, title: str = "Exchange Orderbook"
) -> go.Figure:
    """
    Exchange-style orderbook: bids on left, asks on right, price on Y-axis.

    Layout:
        Qty (Bids) <- | Price | -> Qty (Asks)
                      |  50%  |
        ############# |  48%  | ######
        ##########    |  46%  | ############
    """
    fig = go.Figure()

    bids = book.get("bids", [])
    asks = book.get("asks", [])

    # Bids (green bars extending LEFT - negative x values)
    if bids:
        # Sort bids by price descending (best bid first)
        sorted_bids = sorted(bids, key=lambda x: x["price"], reverse=True)
        bid_prices = [b["price"] * 100 for b in sorted_bids]
        # Use cumulative_qty if available, otherwise calculate
        if sorted_bids and "cumulative_qty" in sorted_bids[0]:
            bid_qty = [-b["cumulative_qty"] for b in sorted_bids]
        else:
            cumulative = 0
            bid_qty = []
            for b in sorted_bids:
                cumulative += b["qty"]
                bid_qty.append(-cumulative)

        fig.add_trace(
            go.Bar(
                y=bid_prices,
                x=bid_qty,
                orientation="h",
                marker_color="rgba(0, 200, 0, 0.6)",
                name="Bids",
                hovertemplate="Price: %{y:.1f}%<br>Cumulative: %{x:,}<extra></extra>",
            )
        )

    # Asks (red bars extending RIGHT - positive x values)
    if asks:
        # Sort asks by price ascending (best ask first)
        sorted_asks = sorted(asks, key=lambda x: x["price"])
        ask_prices = [a["price"] * 100 for a in sorted_asks]
        # Use cumulative_qty if available, otherwise calculate
        if sorted_asks and "cumulative_qty" in sorted_asks[0]:
            ask_qty = [a["cumulative_qty"] for a in sorted_asks]
        else:
            cumulative = 0
            ask_qty = []
            for a in sorted_asks:
                cumulative += a["qty"]
                ask_qty.append(cumulative)

        fig.add_trace(
            go.Bar(
                y=ask_prices,
                x=ask_qty,
                orientation="h",
                marker_color="rgba(255, 0, 0, 0.6)",
                name="Asks",
                hovertemplate="Price: %{y:.1f}%<br>Cumulative: %{x:,}<extra></extra>",
            )
        )

    # Clearing price horizontal line
    if clearing_price is not None:
        fig.add_hline(
            y=clearing_price * 100,
            line_dash="dash",
            line_color="purple",
            line_width=2,
            annotation_text=f"Clearing: {clearing_price * 100:.1f}%",
            annotation_position="right",
        )

    fig.update_layout(
        title=title,
        barmode="overlay",
        xaxis_title="Cumulative Quantity",
        yaxis_title="Price (%)",
        showlegend=True,
        legend=dict(yanchor="top", y=0.99, xanchor="right", x=0.99),
    )

    return fig


def create_comparison_chart(
    initial_book: dict | None, final_book: dict | None, title: str
) -> go.Figure:
    """Create a comparison depth chart showing initial vs final liquidity."""
    fig = go.Figure()

    # Initial asks (lighter red)
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
                    fillcolor="rgba(255, 0, 0, 0.15)",
                    line=dict(color="rgba(255, 0, 0, 0.5)", width=1, dash="dash"),
                    name="Initial Asks",
                )
            )

    # Final asks (solid red)
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
                    fillcolor="rgba(255, 0, 0, 0.3)",
                    line=dict(color="red", width=2),
                    name="Final Asks",
                )
            )

    fig.update_layout(
        title=title,
        xaxis_title="Price (%)",
        yaxis_title="Cumulative Quantity",
        hovermode="x unified",
        showlegend=True,
    )

    return fig


def main():
    st.set_page_config(page_title="Orderbook", page_icon=":book:", layout="wide")
    st.title("Orderbook Depth Analysis")

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    # Check if liquidity data is available
    has_initial = data.initial_liquidity is not None
    has_final = data.final_liquidity is not None
    has_phases = len(data.phase_snapshots) > 0

    if not has_initial and not has_final:
        st.info(
            "No orderbook data available. Run the simulation with the `viz` feature "
            "enabled to capture liquidity snapshots:\n\n"
            "```bash\n"
            "cargo run --bin matching-sim --release --features viz -- --preset small --export-json /tmp/snapshot.json\n"
            "```"
        )
        return

    # Get available markets
    liquidity_source = data.initial_liquidity or data.final_liquidity
    market_names = data.get_book_markets(liquidity_source)

    if not market_names:
        st.warning("No markets found in the liquidity data.")
        return

    # Market selector
    selected_market = st.selectbox(
        "Select Market",
        market_names,
        help="Choose a market to view its orderbook depth",
    )

    if not selected_market:
        return

    st.divider()

    # Phase selector (if available)
    if has_phases:
        st.header("Phase Analysis")

        phase_names = [
            f"{i}: {snap.get('phase', 'Unknown')}" for i, snap in enumerate(data.phase_snapshots)
        ]
        selected_phase_idx = st.select_slider(
            "Select Phase",
            options=list(range(len(data.phase_snapshots))),
            format_func=lambda x: phase_names[x],
            help="Slide through pipeline phases to see liquidity changes",
        )

        phase_snap = data.phase_snapshots[selected_phase_idx]
        col1, col2, col3 = st.columns(3)
        with col1:
            st.metric("Fills", f"{phase_snap.get('fills_count', 0):,}")
        with col2:
            welfare_dollars = phase_snap.get("welfare", 0) / 1e9
            st.metric("Welfare", f"${welfare_dollars:.2f}")
        with col3:
            st.metric("Elapsed", f"{phase_snap.get('elapsed_secs', 0):.3f}s")

        phase_liquidity = data.get_phase_liquidity(selected_phase_idx)
    else:
        phase_liquidity = None

    st.divider()

    # Chart type selector
    chart_type = st.radio(
        "Chart Style",
        ["Exchange (Bids/Asks)", "Traditional Depth"],
        horizontal=True,
        help="Exchange style shows bids left and asks right. Traditional shows cumulative depth.",
    )

    # Get clearing price for this market
    clearing_price = data.get_clearing_price(selected_market, outcome=0)

    st.header(f"Orderbook: {selected_market}")

    # Get books for both outcomes
    if phase_liquidity:
        books = data.get_books_for_market(selected_market, phase_liquidity)
    elif has_initial:
        books = data.get_books_for_market(selected_market, data.initial_liquidity)
    else:
        books = data.get_books_for_market(selected_market, data.final_liquidity)

    if not books:
        st.info("No orderbook data available for this market.")
    else:
        # Create columns for YES and NO outcomes
        cols = st.columns(len(books))
        for i, book in enumerate(sorted(books, key=lambda x: x.get("outcome", 0))):
            outcome_name = "YES" if book.get("outcome", 0) == 0 else "NO"
            outcome = book.get("outcome", 0)

            # Get clearing price for this specific outcome
            outcome_clearing_price = data.get_clearing_price(selected_market, outcome=outcome)

            with cols[i]:
                if chart_type == "Exchange (Bids/Asks)":
                    fig = create_exchange_orderbook(
                        book,
                        clearing_price=outcome_clearing_price,
                        title=f"{outcome_name} Outcome",
                    )
                else:
                    fig = create_depth_chart(book, f"{outcome_name} Outcome")
                st.plotly_chart(fig, width="stretch")

                # Summary stats
                st.subheader("Summary")
                best_ask = book.get("best_ask")
                best_bid = book.get("best_bid")
                spread = book.get("spread")
                mid = book.get("mid_price")

                if best_bid is not None:
                    st.write(f"**Best Bid:** {best_bid * 100:.2f}%")
                if best_ask is not None:
                    st.write(f"**Best Ask:** {best_ask * 100:.2f}%")
                if spread is not None:
                    st.write(f"**Spread:** {spread * 100:.2f}%")
                if mid is not None:
                    st.write(f"**Mid Price:** {mid * 100:.2f}%")
                if outcome_clearing_price is not None:
                    st.write(f"**Clearing Price:** {outcome_clearing_price * 100:.2f}%")

                st.write(f"**Total Bid Qty:** {book.get('total_bid_qty', 0):,}")
                st.write(f"**Total Ask Qty:** {book.get('total_ask_qty', 0):,}")

    # Initial vs Final comparison
    if has_initial and has_final:
        st.divider()
        st.header("Initial vs Final Liquidity")

        initial_books = data.get_books_for_market(selected_market, data.initial_liquidity)
        final_books = data.get_books_for_market(selected_market, data.final_liquidity)

        if initial_books or final_books:
            # Get unique outcomes
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
                    st.plotly_chart(fig, width="stretch")

                    # Calculate consumption
                    initial_qty = (
                        initial_book.get("total_ask_qty", 0) if initial_book else 0
                    )
                    final_qty = final_book.get("total_ask_qty", 0) if final_book else 0
                    consumed = initial_qty - final_qty

                    st.metric(
                        "Liquidity Consumed",
                        f"{consumed:,}",
                        delta=f"-{consumed:,}" if consumed > 0 else None,
                        delta_color="inverse",
                    )

    # Liquidity summary table
    st.divider()
    st.header("All Markets Summary")

    summary_data = []
    liquidity = data.initial_liquidity or data.final_liquidity
    if liquidity:
        for book in liquidity.get("books", []):
            outcome_name = "YES" if book.get("outcome", 0) == 0 else "NO"
            summary_data.append(
                {
                    "Market": book.get("market_name", ""),
                    "Outcome": outcome_name,
                    "Best Ask (%)": f"{book.get('best_ask', 0) * 100:.2f}"
                    if book.get("best_ask")
                    else "-",
                    "Best Bid (%)": f"{book.get('best_bid', 0) * 100:.2f}"
                    if book.get("best_bid")
                    else "-",
                    "Spread (%)": f"{book.get('spread', 0) * 100:.2f}"
                    if book.get("spread")
                    else "-",
                    "Total Ask Qty": book.get("total_ask_qty", 0),
                    "Total Bid Qty": book.get("total_bid_qty", 0),
                }
            )

    if summary_data:
        import pandas as pd

        df = pd.DataFrame(summary_data)
        st.dataframe(df, width="stretch", hide_index=True)


if __name__ == "__main__":
    main()
else:
    main()
