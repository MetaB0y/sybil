"""Markets page: Per-market price evolution and order details."""

import streamlit as st
import plotly.express as px
import plotly.graph_objects as go

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


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
        # Create dual-axis chart for YES/NO prices
        fig = go.Figure()

        fig.add_trace(
            go.Scatter(
                x=price_history["iteration"],
                y=price_history["yes_price"] * 100,
                mode="lines+markers",
                name="YES Price",
                line=dict(color="green"),
            )
        )

        fig.add_trace(
            go.Scatter(
                x=price_history["iteration"],
                y=price_history["no_price"] * 100,
                mode="lines+markers",
                name="NO Price",
                line=dict(color="red"),
            )
        )

        # Add a line showing price sum (should be ~100%)
        fig.add_trace(
            go.Scatter(
                x=price_history["iteration"],
                y=(price_history["yes_price"] + price_history["no_price"]) * 100,
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
        )

        st.plotly_chart(fig, width="stretch")

    # Volume per iteration
    if not price_history.empty and "volume" in price_history.columns:
        st.subheader("Volume per Iteration")

        fig_volume = px.bar(
            price_history,
            x="iteration",
            y="volume",
            title=f"Trading Volume for {selected_market}",
            labels={"iteration": "Iteration", "volume": "Volume (shares)"},
        )
        st.plotly_chart(fig_volume, width="stretch")

    st.divider()

    # Orders for this market
    st.header(f"Orders for {selected_market}")

    orders_df = data.get_orders_for_market(selected_market)

    if orders_df.empty:
        st.info("No orders found for this market.")
    else:
        # Add fill status
        fills_df = data.fills_df()
        filled_order_ids = set(fills_df["order_id"].unique()) if not fills_df.empty else set()

        orders_df["filled"] = orders_df["id"].apply(lambda x: x in filled_order_ids)

        # Display summary stats
        col1, col2, col3 = st.columns(3)
        with col1:
            st.metric("Total Orders", len(orders_df))
        with col2:
            st.metric("Filled Orders", orders_df["filled"].sum())
        with col3:
            st.metric("MM Orders", orders_df["is_mm"].sum())

        # Filter controls
        col1, col2, col3 = st.columns(3)
        with col1:
            filter_type = st.multiselect(
                "Filter by Type",
                orders_df["order_type"].unique().tolist(),
                default=orders_df["order_type"].unique().tolist(),
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
        filtered_df = orders_df[orders_df["order_type"].isin(filter_type)]

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
            width="stretch",
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
