"""Markets page: Per-market price evolution, orderbook depth, and order details."""

import streamlit as st
import plotly.express as px
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
    """
    Create a professional depth chart showing bid/ask liquidity.

    Standard exchange-style depth chart:
    - X-axis: Price
    - Y-axis: Cumulative quantity
    - Bids (green): Build up from right to left (highest bid → lowest)
    - Asks (red): Build up from left to right (lowest ask → highest)
    """
    fig = go.Figure()

    bids = book.get("bids", [])
    asks = book.get("asks", [])

    # Process bids: sort by price descending, cumulate from best bid down
    if bids:
        sorted_bids = sorted(bids, key=lambda x: x["price"], reverse=True)
        cumulative = 0
        bid_prices = []
        bid_cumulative = []
        for b in sorted_bids:
            cumulative += b["qty"]
            bid_prices.append(b["price"] * 100)
            bid_cumulative.append(cumulative)
        # Reverse to show ascending price order for proper fill
        bid_prices = bid_prices[::-1]
        bid_cumulative = bid_cumulative[::-1]

        fig.add_trace(
            go.Scatter(
                x=bid_prices,
                y=bid_cumulative,
                fill="tozeroy",
                fillcolor="rgba(34, 197, 94, 0.4)",  # Tailwind green-500
                line=dict(color="rgb(34, 197, 94)", width=2),
                name="Bids",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative: %{y:,}<extra></extra>",
            )
        )

    # Process asks: sort by price ascending, cumulate from best ask up
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
                fillcolor="rgba(239, 68, 68, 0.4)",  # Tailwind red-500
                line=dict(color="rgb(239, 68, 68)", width=2),
                name="Asks",
                hovertemplate="Price: %{x:.1f}%<br>Cumulative: %{y:,}<extra></extra>",
            )
        )

    # Add clearing price line
    if clearing_price is not None:
        fig.add_vline(
            x=clearing_price * 100,
            line_dash="dash",
            line_color="rgb(168, 85, 247)",  # Tailwind purple-500
            line_width=2,
            annotation_text=f"Clear: {clearing_price * 100:.1f}%",
            annotation_position="top",
            annotation_font_color="rgb(168, 85, 247)",
        )

    fig.update_layout(
        title=dict(text=title, font=dict(size=14)),
        xaxis_title="Price (%)",
        yaxis_title="Cumulative Qty",
        hovermode="x unified",
        showlegend=True,
        legend=dict(
            orientation="h",
            yanchor="bottom",
            y=1.02,
            xanchor="right",
            x=1,
        ),
        height=height,
        margin=dict(l=50, r=20, t=60, b=50),
        xaxis=dict(range=[0, 100]),
    )

    return fig


def render_orderbook_html(
    orders_df,
    fills_df,
    market_name: str,
    clearing_price: float | None = None,
    max_rows: int = 15,
) -> str | None:
    """
    Render a professional exchange-style orderbook as HTML.

    Shows asks (red) on top sorted by price ascending (best ask first),
    bids (green) on bottom sorted by price descending (best bid first).
    """
    if orders_df.empty:
        return None

    # Filter to SINGLE-MARKET orders for this market only
    # Bundle orders have prices for the entire bundle, not per-market, so they're misleading
    market_orders = orders_df[
        (orders_df["markets"] == market_name) &  # Exact match = single market only
        (orders_df["order_type"] == "single")
    ].copy()
    if market_orders.empty:
        return None

    # Get filled order IDs
    filled_ids = set(fills_df["order_id"].unique()) if not fills_df.empty else set()
    market_orders["filled"] = market_orders["id"].isin(filled_ids)

    # Split by side (bid vs ask)
    # "bid" = buying YES at limit_price
    # "ask" = buying NO at limit_price, which is equivalent to selling YES at (1 - limit_price)
    bids = market_orders[market_orders["side"] == "bid"].copy()
    bids = bids.sort_values("limit_price", ascending=False)

    asks = market_orders[market_orders["side"] == "ask"].copy()
    # Convert NO price to YES price: selling YES at (1 - NO_price)
    asks["limit_price"] = 1.0 - asks["limit_price"]
    asks = asks.sort_values("limit_price", ascending=True)

    # Calculate max qty for depth bar scaling
    total_bid_qty = bids["max_qty"].sum() if not bids.empty else 1
    total_ask_qty = asks["max_qty"].sum() if not asks.empty else 1
    max_qty = max(total_bid_qty, total_ask_qty)

    def make_rows(df, color, is_bid, max_rows=15):
        rows = []
        cumulative = 0
        display_df = df.head(max_rows)
        # For asks, we want to show them in reverse order (highest price at top)
        if not is_bid:
            display_df = display_df.iloc[::-1]

        for _, row in display_df.iterrows():
            cumulative += row["max_qty"]
            depth_pct = (cumulative / max_qty * 100) if max_qty > 0 else 0
            price_pct = row["limit_price"] * 100
            qty = row["max_qty"]
            filled_class = "filled" if row["filled"] else "unfilled"
            is_mm = row.get("is_mm", False)
            mm_badge = '<span class="mm-badge">MM</span>' if is_mm else ''

            rows.append(f"""
                <tr class="{filled_class}">
                    <td class="depth-cell" style="--depth: {depth_pct}%; --color: {color};">
                        <span class="price">{price_pct:.2f}%</span>{mm_badge}
                    </td>
                    <td class="qty">{qty:,}</td>
                    <td class="total">{cumulative:,}</td>
                </tr>
            """)
        return "".join(rows)

    ask_rows = make_rows(asks, "rgba(239, 68, 68, 0.4)", is_bid=False, max_rows=max_rows)
    bid_rows = make_rows(bids, "rgba(34, 197, 94, 0.4)", is_bid=True, max_rows=max_rows)

    # Spread calculation
    best_bid = bids["limit_price"].max() if not bids.empty else None
    best_ask = asks["limit_price"].min() if not asks.empty else None
    spread = None
    if best_bid is not None and best_ask is not None:
        spread = best_ask - best_bid

    # Show spread/clearing price
    spread_html = ""
    if spread is not None or clearing_price is not None:
        spread_text = f"Spread: {spread * 100:.2f}%" if spread is not None else ""
        clearing_text = f"Clearing: {clearing_price * 100:.2f}%" if clearing_price is not None else ""
        divider = " | " if spread_text and clearing_text else ""
        spread_html = f"""
        <div class="spread-row">
            {spread_text}{divider}{clearing_text}
        </div>
        """

    # Count filled/unfilled and MM
    bids_filled = bids["filled"].sum() if not bids.empty else 0
    asks_filled = asks["filled"].sum() if not asks.empty else 0
    bids_mm = bids["is_mm"].sum() if not bids.empty and "is_mm" in bids.columns else 0
    asks_mm = asks["is_mm"].sum() if not asks.empty and "is_mm" in asks.columns else 0

    html = f"""
    <style>
        .orderbook {{
            font-family: 'SF Mono', 'Monaco', 'Consolas', monospace;
            font-size: 13px;
            width: 100%;
            border-collapse: collapse;
        }}
        .orderbook th {{
            text-align: right;
            padding: 6px 12px;
            color: #888;
            font-weight: 500;
            font-size: 11px;
            text-transform: uppercase;
            border-bottom: 1px solid #333;
        }}
        .orderbook td {{
            text-align: right;
            padding: 4px 12px;
        }}
        .orderbook .depth-cell {{
            position: relative;
            text-align: left;
        }}
        .orderbook .depth-cell::before {{
            content: '';
            position: absolute;
            right: 0;
            top: 0;
            bottom: 0;
            width: var(--depth);
            background: var(--color);
            z-index: 0;
        }}
        .orderbook .depth-cell .price {{
            position: relative;
            z-index: 1;
        }}
        .orderbook .qty, .orderbook .total {{
            color: #ccc;
        }}
        .orderbook tr.unfilled td {{
            opacity: 0.5;
        }}
        .mm-badge {{
            display: inline-block;
            background: rgba(168, 85, 247, 0.3);
            color: rgb(168, 85, 247);
            font-size: 9px;
            font-weight: 600;
            padding: 1px 4px;
            border-radius: 3px;
            margin-left: 6px;
            vertical-align: middle;
        }}
        .orderbook-section {{
            margin-bottom: 0;
        }}
        .section-label {{
            font-size: 11px;
            text-transform: uppercase;
            padding: 4px 12px;
            background: #1a1a1a;
            display: flex;
            justify-content: space-between;
        }}
        .section-label.asks {{
            color: rgb(239, 68, 68);
        }}
        .section-label.bids {{
            color: rgb(34, 197, 94);
        }}
        .section-label .stats {{
            color: #666;
            font-weight: normal;
        }}
        .spread-row {{
            text-align: center;
            padding: 8px;
            background: rgba(168, 85, 247, 0.15);
            color: rgb(168, 85, 247);
            font-weight: 600;
            font-size: 12px;
            border-top: 1px solid #333;
            border-bottom: 1px solid #333;
        }}
        .orderbook-container {{
            background: #0e0e0e;
            border-radius: 8px;
            overflow: hidden;
            border: 1px solid #333;
        }}
    </style>
    <div class="orderbook-container">
        <div class="orderbook-section">
            <div class="section-label asks">
                <span>Asks (Sell YES) - Single-market only</span>
                <span class="stats">{len(asks)} orders, {asks_filled} filled, {asks_mm} MM</span>
            </div>
            <table class="orderbook">
                <thead>
                    <tr>
                        <th style="text-align: left;">Price</th>
                        <th>Qty</th>
                        <th>Total</th>
                    </tr>
                </thead>
                <tbody>
                    {ask_rows if ask_rows else '<tr><td colspan="3" style="text-align:center;color:#666;padding:12px;">No asks</td></tr>'}
                </tbody>
            </table>
        </div>
        {spread_html}
        <div class="orderbook-section">
            <div class="section-label bids">
                <span>Bids (Buy YES) - Single-market only</span>
                <span class="stats">{len(bids)} orders, {bids_filled} filled, {bids_mm} MM</span>
            </div>
            <table class="orderbook">
                <thead>
                    <tr>
                        <th style="text-align: left;">Price</th>
                        <th>Qty</th>
                        <th>Total</th>
                    </tr>
                </thead>
                <tbody>
                    {bid_rows if bid_rows else '<tr><td colspan="3" style="text-align:center;color:#666;padding:12px;">No bids</td></tr>'}
                </tbody>
            </table>
        </div>
    </div>
    """
    return html


def main():
    st.set_page_config(page_title="Markets", page_icon=":chart_with_upwards_trend:", layout="wide")
    st.title("Market Analysis")

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    # Get market names
    market_names = data.get_market_names()
    if not market_names:
        st.warning("No markets found in the snapshot.")
        return

    # Market selector
    selected_market = st.selectbox(
        "Select Market",
        market_names,
        help="Choose a market to view its price evolution and orders",
    )

    if not selected_market:
        return

    st.divider()

    # Price evolution chart
    st.header(f"Price Evolution: {selected_market}")

    price_history = data.get_market_price_history(selected_market)

    if price_history.empty:
        st.info("No price history available for this market.")
    else:
        # Build continuous price series: point 0 = before iter 1, point N = after iter N
        # This avoids redundant start/end lines (end of iter N = start of iter N+1)
        has_end_prices = "yes_price_end" in price_history.columns

        if has_end_prices and len(price_history) > 0:
            # Build x-axis: 0, 1, 2, ..., N (N+1 points for N iterations)
            x_points = [0]  # Point 0 = before any iteration
            yes_points = [price_history.iloc[0]["yes_price"] * 100]
            no_points = [price_history.iloc[0]["no_price"] * 100]

            for _, row in price_history.iterrows():
                x_points.append(int(row["iteration"]))
                yes_points.append(row["yes_price_end"] * 100)
                no_points.append(row["no_price_end"] * 100)
        else:
            # Fallback for old snapshots
            x_points = price_history["iteration"].tolist()
            yes_points = (price_history["yes_price"] * 100).tolist()
            no_points = (price_history["no_price"] * 100).tolist()

        fig = go.Figure()

        fig.add_trace(
            go.Scatter(
                x=x_points,
                y=yes_points,
                mode="lines+markers",
                name="YES Price",
                line=dict(color="rgb(34, 197, 94)"),
                marker=dict(size=6),
            )
        )

        fig.add_trace(
            go.Scatter(
                x=x_points,
                y=no_points,
                mode="lines+markers",
                name="NO Price",
                line=dict(color="rgb(239, 68, 68)"),
                marker=dict(size=6),
            )
        )

        # Add price sum line
        sum_points = [y + n for y, n in zip(yes_points, no_points)]
        fig.add_trace(
            go.Scatter(
                x=x_points,
                y=sum_points,
                mode="lines",
                name="Sum",
                line=dict(color="gray", dash="dash"),
            )
        )

        fig.update_layout(
            title=f"Price Evolution for {selected_market}",
            xaxis_title="Iteration",
            yaxis_title="Price (%)",
            hovermode="x unified",
            yaxis=dict(range=[0, 110]),
            xaxis=dict(tickmode="linear", tick0=0, dtick=1),  # Force integer ticks
        )

        st.plotly_chart(fig, use_container_width=True)

    # Volume per iteration (cumulative)
    if not price_history.empty and "volume" in price_history.columns:
        st.subheader("Cumulative Volume")

        fig_volume = px.bar(
            price_history,
            x="iteration",
            y="volume",
            title=f"Cumulative Trading Volume for {selected_market}",
            labels={"iteration": "Iteration", "volume": "Volume (shares)"},
        )
        fig_volume.update_layout(
            xaxis=dict(tickmode="linear", tick0=1, dtick=1),  # Force integer ticks
        )
        st.plotly_chart(fig_volume, use_container_width=True)

    st.divider()

    # Demand/Supply curve from orders (the main visualization)
    st.header(f"Order Book: {selected_market}")

    orders_df = data.orders_df()
    fills_df = data.fills_df()

    # Show clearing price prominently
    clearing_price_yes = data.get_clearing_price(selected_market, outcome=0)
    clearing_price_no = data.get_clearing_price(selected_market, outcome=1)

    if clearing_price_yes is not None or clearing_price_no is not None:
        col1, col2 = st.columns(2)
        with col1:
            if clearing_price_yes is not None:
                st.metric("YES Clearing Price", f"{clearing_price_yes * 100:.1f}%")
        with col2:
            if clearing_price_no is not None:
                st.metric("NO Clearing Price", f"{clearing_price_no * 100:.1f}%")

    # Render exchange-style orderbook
    orderbook_html = render_orderbook_html(
        orders_df,
        fills_df,
        selected_market,
        clearing_price=clearing_price_yes,
        max_rows=20,
    )

    if orderbook_html:
        # st.html was added in Streamlit 1.33, fallback to markdown for older versions
        if hasattr(st, "html"):
            st.html(orderbook_html)
        else:
            st.markdown(orderbook_html, unsafe_allow_html=True)
    else:
        st.info("No orders found for this market.")

    # MM Liquidity section (collapsible since it's supplementary info)
    has_liquidity = data.initial_liquidity is not None or data.final_liquidity is not None

    if has_liquidity:
        with st.expander("MM Liquidity (Market Maker Quotes)", expanded=False):
            st.caption(
                "Shows market maker bid/ask quotes. Note: MM quotes are often far from "
                "the clearing price since user orders may dominate price discovery."
            )

            liquidity = data.initial_liquidity or data.final_liquidity
            books = data.get_books_for_market(selected_market, liquidity)

            if books:
                cols = st.columns(len(books))
                for i, book in enumerate(sorted(books, key=lambda x: x.get("outcome", 0))):
                    outcome = book.get("outcome", 0)
                    outcome_name = "YES" if outcome == 0 else "NO"

                    with cols[i]:
                        # Don't show clearing price on MM chart - different concept
                        fig = create_depth_chart(
                            book,
                            clearing_price=None,  # Don't mix concepts
                            title=f"{outcome_name} MM Liquidity",
                            height=250,
                        )
                        st.plotly_chart(fig, use_container_width=True)

                        # Quick stats
                        best_bid = book.get("best_bid")
                        best_ask = book.get("best_ask")
                        spread = book.get("spread")

                        stats_cols = st.columns(3)
                        with stats_cols[0]:
                            if best_bid is not None:
                                st.metric("MM Bid", f"{best_bid * 100:.1f}%")
                        with stats_cols[1]:
                            if best_ask is not None:
                                st.metric("MM Ask", f"{best_ask * 100:.1f}%")
                        with stats_cols[2]:
                            if spread is not None:
                                st.metric("Spread", f"{spread * 100:.1f}%")
            else:
                st.info("No MM liquidity data for this market.")

    st.divider()

    # Orders for this market
    st.header(f"Orders for {selected_market}")

    orders_for_market = data.get_orders_for_market(selected_market)

    if orders_for_market.empty:
        st.info("No orders found for this market.")
    else:
        # Add fill status
        filled_order_ids = set(fills_df["order_id"].unique()) if not fills_df.empty else set()

        orders_for_market["filled"] = orders_for_market["id"].apply(lambda x: x in filled_order_ids)

        # Display summary stats
        col1, col2, col3 = st.columns(3)
        with col1:
            st.metric("Total Orders", len(orders_for_market))
        with col2:
            st.metric("Filled Orders", orders_for_market["filled"].sum())
        with col3:
            st.metric("MM Orders", orders_for_market["is_mm"].sum())

        # Filter controls
        col1, col2, col3 = st.columns(3)
        with col1:
            filter_type = st.multiselect(
                "Filter by Type",
                orders_for_market["order_type"].unique().tolist(),
                default=orders_for_market["order_type"].unique().tolist(),
            )
        with col2:
            filter_filled = st.selectbox(
                "Filter by Fill Status",
                ["All", "Filled Only", "Unfilled Only"],
            )
        with col3:
            filter_mm = st.selectbox(
                "Filter by MM Status",
                ["All", "MM Only", "User Only"],
            )

        # Apply filters
        filtered_df = orders_for_market[orders_for_market["order_type"].isin(filter_type)]

        if filter_filled == "Filled Only":
            filtered_df = filtered_df[filtered_df["filled"]]
        elif filter_filled == "Unfilled Only":
            filtered_df = filtered_df[~filtered_df["filled"]]

        if filter_mm == "MM Only":
            filtered_df = filtered_df[filtered_df["is_mm"]]
        elif filter_mm == "User Only":
            filtered_df = filtered_df[~filtered_df["is_mm"]]

        # Format for display
        display_df = filtered_df.copy()
        display_df["limit_price"] = display_df["limit_price"].apply(lambda x: f"${x:.4f}")

        st.dataframe(
            display_df[
                ["id", "markets", "order_type", "is_aon", "is_mm", "limit_price", "max_qty", "filled"]
            ],
            use_container_width=True,
            hide_index=True,
        )

        # Link to order details
        if not filtered_df.empty:
            st.subheader("View Order Details")
            selected_order_id = st.selectbox(
                "Select an order to view details",
                filtered_df["id"].tolist(),
                format_func=lambda x: f"Order #{x}",
            )

            if selected_order_id:
                # Store in session state for orders page
                st.session_state["selected_order_id"] = selected_order_id
                st.page_link("pages/3_orders.py", label=f"Go to Order #{selected_order_id} Details")


if __name__ == "__main__":
    main()
else:
    main()
