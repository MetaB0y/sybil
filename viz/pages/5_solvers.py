"""Solvers page: Per-phase statistics and solver contributions."""

import streamlit as st
import pandas as pd
import plotly.express as px
import plotly.graph_objects as go

from utils.loader import SnapshotData


def get_data() -> SnapshotData | None:
    """Get snapshot data from session state."""
    return st.session_state.get("snapshot")


def format_phase_name(phase: str | dict) -> str:
    """Format phase name from serialized enum."""
    if isinstance(phase, dict):
        # Handle enum serialization like {"PriceDiscovery": null}
        return list(phase.keys())[0] if phase else "Unknown"
    return str(phase)


def main():
    st.set_page_config(page_title="Solvers", page_icon=":gear:", layout="wide")
    st.title("Solver Analysis")

    data = get_data()
    if data is None:
        st.warning("No snapshot loaded. Please load a snapshot from the main page.")
        return

    # Check if phase snapshots are available
    has_phases = len(data.phase_snapshots) > 0

    if not has_phases:
        st.info(
            "No phase snapshot data available. Run the simulation with the `viz` feature "
            "enabled to capture per-phase snapshots:\n\n"
            "```bash\n"
            "cargo run --bin matching-sim --release --features viz -- --preset small --export-json /tmp/snapshot.json\n"
            "```"
        )

    # Phase Progression Section
    if has_phases:
        st.header("Phase Progression")

        phase_df = data.phases_df()

        if not phase_df.empty:
            # Summary metrics
            col1, col2, col3, col4 = st.columns(4)
            with col1:
                total_phases = len(phase_df)
                st.metric("Total Phases", total_phases)
            with col2:
                max_iter = phase_df["iteration"].max()
                st.metric("Iterations", max_iter)
            with col3:
                final_fills = phase_df["fills_count"].iloc[-1] if len(phase_df) > 0 else 0
                st.metric("Final Fills", f"{final_fills:,}")
            with col4:
                final_welfare = phase_df["welfare_dollars"].iloc[-1] if len(phase_df) > 0 else 0
                st.metric("Final Welfare", f"${final_welfare:.2f}")

            st.divider()

            # Phase table with phase-specific data
            # fills_count = cumulative fills if stopped at this phase
            # phase_fills = fills added by this specific phase (delta)
            st.subheader("Phase Statistics")
            display_cols = ["phase", "iteration", "phase_fills", "fills_count", "phase_welfare_dollars", "metadata", "elapsed_secs"]
            display_df = phase_df[display_cols].copy()
            display_df.columns = ["Phase", "Iter", "Phase Δ", "Cumulative", "Phase Welfare ($)", "Details", "Time (s)"]
            # Format phase_fills: show "-" for None
            display_df["Phase Δ"] = display_df["Phase Δ"].apply(lambda x: f"+{x:,}" if x is not None and x > 0 else ("-" if x is None else f"{x:,}"))
            display_df["Cumulative"] = display_df["Cumulative"].apply(lambda x: f"{x:,}")
            display_df["Phase Welfare ($)"] = display_df["Phase Welfare ($)"].apply(lambda x: f"${x:.2f}" if x else "-")
            st.dataframe(display_df, width="stretch", hide_index=True)

            st.divider()

            # Welfare progression chart
            st.subheader("Welfare by Phase")
            fig_welfare = px.line(
                phase_df,
                x="index",
                y="welfare_dollars",
                markers=True,
                labels={"index": "Phase Index", "welfare_dollars": "Welfare ($)"},
                hover_data=["phase", "iteration", "fills_count"],
            )
            fig_welfare.update_layout(
                xaxis=dict(
                    tickmode="array",
                    tickvals=phase_df["index"].tolist(),
                    ticktext=phase_df["phase"].tolist(),
                ),
            )
            st.plotly_chart(fig_welfare, width="stretch")

            # Fills progression chart
            st.subheader("Cumulative Fills by Phase")
            fig_fills = px.bar(
                phase_df,
                x="phase",
                y="fills_count",
                color="iteration",
                labels={"phase": "Phase", "fills_count": "Cumulative Fills", "iteration": "Iteration"},
                hover_data=["welfare_dollars", "elapsed_secs", "phase_fills"],
            )
            st.plotly_chart(fig_fills, width="stretch")

            # Time breakdown
            st.subheader("Time Spent per Phase")

            # Group by phase type and sum time
            time_by_phase = phase_df.groupby("phase")["elapsed_secs"].max().reset_index()
            time_by_phase.columns = ["Phase", "Time (s)"]

            fig_time = px.pie(
                time_by_phase,
                values="Time (s)",
                names="Phase",
                title="Time Distribution by Phase",
            )
            st.plotly_chart(fig_time, width="stretch")

    st.divider()

    # Solver Contributions Section
    st.header("Solver Contributions")

    iterations_df = data.iterations_df()
    if not iterations_df.empty:
        # Summary of fills by source
        total_pd_fills = iterations_df["price_discovery_fills"].sum()
        total_bundle_fills = iterations_df["bundle_fills"].sum()

        col1, col2, col3 = st.columns(3)
        with col1:
            st.metric("Price Discovery Fills", f"{total_pd_fills:,}")
        with col2:
            st.metric("Bundle/Arbitrage Fills", f"{total_bundle_fills:,}")
        with col3:
            total = total_pd_fills + total_bundle_fills
            pd_pct = (total_pd_fills / total * 100) if total > 0 else 0
            st.metric("Price Discovery %", f"{pd_pct:.1f}%")

        st.divider()

        # Fills by iteration breakdown
        st.subheader("Fills by Iteration")

        # Melt data for stacked bar chart
        fills_data = iterations_df[["iteration", "price_discovery_fills", "bundle_fills"]].melt(
            id_vars=["iteration"],
            value_vars=["price_discovery_fills", "bundle_fills"],
            var_name="Source",
            value_name="Fills",
        )
        fills_data["Source"] = fills_data["Source"].map({
            "price_discovery_fills": "Price Discovery",
            "bundle_fills": "Bundle/Arbitrage",
        })

        fig_fills_iter = px.bar(
            fills_data,
            x="iteration",
            y="Fills",
            color="Source",
            barmode="stack",
            labels={"iteration": "Iteration", "Fills": "Number of Fills"},
        )
        st.plotly_chart(fig_fills_iter, width="stretch")

        # Welfare progression by iteration
        st.subheader("Welfare by Iteration")
        fig_welfare_iter = px.line(
            iterations_df,
            x="iteration",
            y="welfare_dollars",
            markers=True,
            labels={"iteration": "Iteration", "welfare_dollars": "Welfare ($)"},
        )
        st.plotly_chart(fig_welfare_iter, width="stretch")

        # Delta analysis
        st.subheader("Convergence Analysis")

        fig_delta = go.Figure()
        fig_delta.add_trace(
            go.Bar(
                x=iterations_df["iteration"],
                y=iterations_df["welfare_delta_dollars"],
                name="Welfare Delta ($)",
                marker_color=["green" if d > 0 else "red" for d in iterations_df["welfare_delta_dollars"]],
            )
        )
        fig_delta.update_layout(
            xaxis_title="Iteration",
            yaxis_title="Welfare Delta ($)",
            showlegend=False,
        )
        st.plotly_chart(fig_delta, width="stretch")
    else:
        st.info("No iteration data available.")

    st.divider()

    # Phase Times from config
    st.header("Pipeline Timing Breakdown")

    phase_times = data.phase_times
    if phase_times:
        times_data = {
            "Phase": ["Price Discovery", "Negrisk Arbitrage", "Allocation", "Partial Solving", "Combining"],
            "Time (s)": [
                phase_times.get("price_discovery_secs", 0),
                phase_times.get("negrisk_secs", 0),
                phase_times.get("allocation_secs", 0),
                phase_times.get("partial_solving_secs", 0),
                phase_times.get("combining_secs", 0),
            ],
        }
        times_df = pd.DataFrame(times_data)
        times_df = times_df[times_df["Time (s)"] > 0]  # Filter out zero times

        if not times_df.empty:
            col1, col2 = st.columns(2)
            with col1:
                fig_times = px.bar(
                    times_df,
                    x="Phase",
                    y="Time (s)",
                    title="Time by Pipeline Phase",
                )
                st.plotly_chart(fig_times, width="stretch")
            with col2:
                fig_pie = px.pie(
                    times_df,
                    values="Time (s)",
                    names="Phase",
                    title="Time Distribution",
                )
                st.plotly_chart(fig_pie, width="stretch")

            total_time = phase_times.get("total_secs", sum(times_df["Time (s)"]))
            st.metric("Total Pipeline Time", f"{total_time:.3f}s")
    else:
        st.info("No phase timing data available.")


if __name__ == "__main__":
    main()
else:
    main()
