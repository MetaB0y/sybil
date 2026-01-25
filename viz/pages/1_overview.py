"""Overview page: Convergence charts, phase timing, summary stats."""

import streamlit as st
import plotly.express as px
import plotly.graph_objects as go
from plotly.subplots import make_subplots

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


def main():
    st.set_page_config(page_title="Overview", page_icon=":bar_chart:", layout="wide")
    st.title("Pipeline Overview")

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    # Iteration slider
    iterations_df = data.iterations_df()
    if iterations_df.empty:
        st.warning("No iteration data available.")
        return

    max_iter = int(iterations_df["iteration"].max())
    selected_iter = st.slider(
        "Select Iteration",
        min_value=1,
        max_value=max_iter,
        value=max_iter,
        help="Slide to see the state at different iterations",
    )

    st.divider()

    # Convergence charts
    st.header("Convergence")

    col1, col2 = st.columns(2)

    with col1:
        # Welfare convergence
        fig_welfare = px.line(
            iterations_df,
            x="iteration",
            y="welfare_dollars",
            title="Welfare Convergence",
            labels={"iteration": "Iteration", "welfare_dollars": "Welfare ($)"},
            markers=True,
        )
        fig_welfare.add_vline(
            x=selected_iter, line_dash="dash", line_color="red", opacity=0.5
        )
        st.plotly_chart(fig_welfare, width="stretch")

    with col2:
        # Volume convergence
        fig_volume = px.line(
            iterations_df,
            x="iteration",
            y="volume",
            title="Volume Convergence",
            labels={"iteration": "Iteration", "volume": "Volume (shares)"},
            markers=True,
        )
        fig_volume.add_vline(
            x=selected_iter, line_dash="dash", line_color="red", opacity=0.5
        )
        st.plotly_chart(fig_volume, width="stretch")

    # Delta charts
    col3, col4 = st.columns(2)

    with col3:
        # Welfare delta
        fig_delta = px.bar(
            iterations_df[iterations_df["iteration"] > 1],
            x="iteration",
            y="welfare_delta_dollars",
            title="Welfare Delta per Iteration",
            labels={"iteration": "Iteration", "welfare_delta_dollars": "Delta ($)"},
        )
        st.plotly_chart(fig_delta, width="stretch")

    with col4:
        # Fills breakdown
        fig_fills = go.Figure()
        fig_fills.add_trace(
            go.Bar(
                x=iterations_df["iteration"],
                y=iterations_df["price_discovery_fills"],
                name="Price Discovery",
            )
        )
        fig_fills.add_trace(
            go.Bar(
                x=iterations_df["iteration"],
                y=iterations_df["bundle_fills"],
                name="Bundle Matching",
            )
        )
        fig_fills.update_layout(
            title="Fills by Source per Iteration",
            xaxis_title="Iteration",
            yaxis_title="Fills",
            barmode="stack",
        )
        st.plotly_chart(fig_fills, width="stretch")

    st.divider()

    # Phase timing
    st.header("Phase Timing Breakdown")

    phase_times = data.phase_times
    timing_data = {
        "Phase": [
            "Price Discovery",
            "Negrisk Arbitrage",
            "MM Allocation",
            "Bundle Matching",
            "Combining",
        ],
        "Time (s)": [
            phase_times.get("price_discovery_secs", 0),
            phase_times.get("negrisk_secs", 0),
            phase_times.get("allocation_secs", 0),
            phase_times.get("partial_solving_secs", 0),
            phase_times.get("combining_secs", 0),
        ],
    }

    # Filter out zero times
    filtered_phases = []
    filtered_times = []
    for phase, time in zip(timing_data["Phase"], timing_data["Time (s)"]):
        if time > 0:
            filtered_phases.append(phase)
            filtered_times.append(time)

    if filtered_phases:
        fig_timing = px.pie(
            names=filtered_phases,
            values=filtered_times,
            title="Time Distribution by Phase",
            hole=0.3,
        )
        st.plotly_chart(fig_timing, width="stretch")
    else:
        st.info("No timing data available.")

    st.divider()

    # Summary stats table for selected iteration
    st.header(f"Iteration {selected_iter} Details")

    iter_row = iterations_df[iterations_df["iteration"] == selected_iter].iloc[0]

    col1, col2, col3, col4 = st.columns(4)
    with col1:
        st.metric("Welfare", f"${iter_row['welfare_dollars']:.2f}")
    with col2:
        st.metric("Volume", f"{iter_row['volume']:,}")
    with col3:
        st.metric("Fills", f"{iter_row['fills']:,}")
    with col4:
        delta = iter_row["welfare_delta_dollars"]
        st.metric("Welfare Delta", f"${delta:.2f}" if delta != 0 else "-")

    # Market prices at selected iteration
    st.subheader("Market Prices")
    market_prices_df = data.market_prices_df(selected_iter)
    if not market_prices_df.empty:
        # Format for display
        display_df = market_prices_df.copy()
        display_df["yes_price"] = display_df["yes_price"].apply(lambda x: f"{x:.2%}")
        display_df["no_price"] = display_df["no_price"].apply(lambda x: f"{x:.2%}")
        st.dataframe(display_df, width="stretch", hide_index=True)
    else:
        st.info("No market price data for this iteration.")


if __name__ == "__main__":
    main()
else:
    main()
