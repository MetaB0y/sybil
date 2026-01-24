"""Orders page: Individual order details and fill history."""

import streamlit as st
import plotly.express as px
import plotly.graph_objects as go

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


def main():
    st.set_page_config(page_title="Orders", page_icon=":clipboard:", layout="wide")
    st.title("Order Analysis")

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    orders_df = data.orders_df()
    if orders_df.empty:
        st.warning("No orders found in the snapshot.")
        return

    # Get order IDs
    order_ids = orders_df["id"].tolist()

    # Check if an order was selected from the markets page
    default_order_id = st.session_state.get("selected_order_id", order_ids[0] if order_ids else None)

    # Order selector
    selected_order_id = st.selectbox(
        "Select Order",
        order_ids,
        index=order_ids.index(default_order_id) if default_order_id in order_ids else 0,
        format_func=lambda x: f"Order #{x}",
        help="Choose an order to view its details and fill history",
    )

    if not selected_order_id:
        return

    # Get order details
    order_row = orders_df[orders_df["id"] == selected_order_id].iloc[0]

    st.divider()

    # Order details card
    st.header(f"Order #{selected_order_id} Details")

    col1, col2, col3 = st.columns(3)

    with col1:
        st.subheader("Basic Info")
        st.write(f"**Type:** {order_row['order_type'].title()}")
        st.write(f"**Markets:** {order_row['markets']}")
        st.write(f"**Limit Price:** ${order_row['limit_price']:.4f}")
        st.write(f"**Max Quantity:** {order_row['max_qty']:,}")

    with col2:
        st.subheader("Flags")
        aon_status = "Yes" if order_row["is_aon"] else "No"
        mm_status = "Yes" if order_row["is_mm"] else "No"
        st.write(f"**All-or-None:** {aon_status}")
        st.write(f"**Market Maker:** {mm_status}")

    with col3:
        # Fill summary
        fills_df = data.get_order_fills(selected_order_id)
        if not fills_df.empty:
            total_filled = fills_df["fill_qty"].sum()
            total_welfare = fills_df["welfare"].sum()
            avg_price = fills_df["fill_price"].mean()

            st.subheader("Fill Summary")
            st.write(f"**Filled Quantity:** {total_filled:,}")
            st.write(f"**Fill Rate:** {total_filled / order_row['max_qty'] * 100:.1f}%")
            st.write(f"**Avg Fill Price:** ${avg_price:.4f}")
            st.write(f"**Total Welfare:** ${total_welfare:.4f}")
        else:
            st.subheader("Fill Summary")
            st.write("**Status:** Not filled")

    st.divider()

    # Fill history
    st.header("Fill History")

    fills_df = data.get_order_fills(selected_order_id)

    if fills_df.empty:
        st.info("This order has not been filled.")
    else:
        # Fill history table
        st.subheader("Fill Records")

        display_fills = fills_df.copy()
        display_fills["fill_price"] = display_fills["fill_price"].apply(lambda x: f"${x:.4f}")
        display_fills["welfare"] = display_fills["welfare"].apply(lambda x: f"${x:.4f}")

        st.dataframe(display_fills, width="stretch", hide_index=True)

        # Fill timeline chart
        if len(fills_df) > 1:
            st.subheader("Fill Timeline")

            fig = go.Figure()

            # Cumulative fill quantity
            fills_df["cumulative_qty"] = fills_df["fill_qty"].cumsum()

            fig.add_trace(
                go.Scatter(
                    x=fills_df["iteration"],
                    y=fills_df["cumulative_qty"],
                    mode="lines+markers",
                    name="Cumulative Fill",
                    fill="tozeroy",
                )
            )

            fig.add_hline(
                y=order_row["max_qty"],
                line_dash="dash",
                line_color="red",
                annotation_text="Max Qty",
            )

            fig.update_layout(
                title="Cumulative Fill Over Iterations",
                xaxis_title="Iteration",
                yaxis_title="Filled Quantity",
            )

            st.plotly_chart(fig, width="stretch")

        # Welfare contribution chart
        st.subheader("Welfare Contribution")

        fig_welfare = px.bar(
            fills_df,
            x="iteration",
            y="welfare",
            title="Welfare Contribution per Iteration",
            labels={"iteration": "Iteration", "welfare": "Welfare ($)"},
            color="source",
        )
        st.plotly_chart(fig_welfare, width="stretch")

    st.divider()

    # All orders summary table
    st.header("All Orders Summary")

    # Add fill status to orders
    all_fills = data.fills_df()
    if not all_fills.empty:
        fill_summary = all_fills.groupby("order_id").agg(
            filled_qty=("fill_qty", "sum"),
            avg_price=("fill_price", "mean"),
            total_welfare=("welfare", "sum"),
        ).reset_index()

        orders_with_fills = orders_df.merge(
            fill_summary, left_on="id", right_on="order_id", how="left"
        )
        orders_with_fills["filled_qty"] = orders_with_fills["filled_qty"].fillna(0)
        orders_with_fills["fill_rate"] = orders_with_fills["filled_qty"] / orders_with_fills["max_qty"]
    else:
        orders_with_fills = orders_df.copy()
        orders_with_fills["filled_qty"] = 0
        orders_with_fills["fill_rate"] = 0.0

    # Filter controls
    col1, col2 = st.columns(2)
    with col1:
        filter_type = st.multiselect(
            "Filter by Type",
            orders_df["order_type"].unique().tolist(),
            default=orders_df["order_type"].unique().tolist(),
            key="order_filter_type",
        )
    with col2:
        filter_status = st.selectbox(
            "Filter by Status",
            ["All", "Filled", "Unfilled"],
            key="order_filter_status",
        )

    # Apply filters
    filtered = orders_with_fills[orders_with_fills["order_type"].isin(filter_type)]
    if filter_status == "Filled":
        filtered = filtered[filtered["filled_qty"] > 0]
    elif filter_status == "Unfilled":
        filtered = filtered[filtered["filled_qty"] == 0]

    # Format for display
    display = filtered.copy()
    display["limit_price"] = display["limit_price"].apply(lambda x: f"${x:.4f}")
    display["fill_rate"] = display["fill_rate"].apply(lambda x: f"{x * 100:.1f}%")

    columns_to_show = ["id", "markets", "order_type", "is_aon", "is_mm", "limit_price", "max_qty", "filled_qty", "fill_rate"]
    columns_to_show = [c for c in columns_to_show if c in display.columns]

    st.dataframe(
        display[columns_to_show],
        width="stretch",
        hide_index=True,
    )


if __name__ == "__main__":
    main()
else:
    main()
