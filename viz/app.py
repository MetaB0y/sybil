"""Sybil Matching Pipeline Visualization Dashboard.

Usage:
    streamlit run app.py -- /path/to/snapshot.json

This dashboard provides interactive visualization of pipeline results:
- Overview: Convergence charts, phase timing, summary stats
- Markets: Per-market price evolution and volume
- Orders: Individual order fill history and details
"""

import sys
from pathlib import Path

import streamlit as st

# Add utils to path
sys.path.insert(0, str(Path(__file__).parent))

from utils.loader import load_snapshot


def main():
    st.set_page_config(
        page_title="Sybil Pipeline Visualizer",
        page_icon=":chart_with_upwards_trend:",
        layout="wide",
    )

    st.title("Sybil Matching Pipeline Visualizer")

    # Get snapshot path from command line or file uploader
    snapshot_path = None

    # Check command line args (after --)
    if len(sys.argv) > 1:
        arg_path = Path(sys.argv[1])
        if arg_path.exists():
            snapshot_path = arg_path

    # Also allow file upload
    uploaded_file = st.sidebar.file_uploader(
        "Upload Snapshot JSON", type=["json"], help="Upload a pipeline snapshot JSON file"
    )

    if uploaded_file is not None:
        import tempfile

        with tempfile.NamedTemporaryFile(delete=False, suffix=".json") as f:
            f.write(uploaded_file.getvalue())
            snapshot_path = Path(f.name)

    if snapshot_path is None:
        st.info(
            """
            No snapshot loaded. You can either:
            1. Run with: `streamlit run app.py -- /path/to/snapshot.json`
            2. Upload a JSON file using the sidebar
            3. Generate a snapshot with: `cargo run --bin matching-sim --release -- --preset small --export-json /tmp/snap.json`
            """
        )
        return

    # Load the snapshot
    try:
        data = load_snapshot(snapshot_path)
        st.session_state["snapshot"] = data
        st.session_state["snapshot_path"] = str(snapshot_path)
    except Exception as e:
        st.error(f"Failed to load snapshot: {e}")
        return

    # Display summary in sidebar
    st.sidebar.header("Snapshot Info")
    st.sidebar.write(f"**Scenario:** {data.scenario_name}")
    st.sidebar.write(f"**Markets:** {data.config.get('num_markets', 0)}")
    st.sidebar.write(f"**Orders:** {data.config.get('num_orders', 0)}")
    st.sidebar.write(f"**MM Constraints:** {data.config.get('num_mm_constraints', 0)}")
    st.sidebar.write(f"**Iterations:** {data.config.get('pipeline_iterations', 0)}")

    # Final result summary
    st.sidebar.header("Final Result")
    final = data.final_result
    st.sidebar.metric("Total Welfare", f"${final.get('total_welfare_dollars', 0):.2f}")
    st.sidebar.metric("Total Volume", f"{final.get('total_volume', 0):,}")
    st.sidebar.metric(
        "Fill Rate", f"{final.get('fill_rate', 0) * 100:.1f}%"
    )

    # Main content
    st.header("Quick Summary")

    col1, col2, col3, col4 = st.columns(4)
    with col1:
        st.metric("Total Welfare", f"${final.get('total_welfare_dollars', 0):.2f}")
    with col2:
        st.metric("Total Volume", f"{final.get('total_volume', 0):,}")
    with col3:
        st.metric("Orders Filled", f"{final.get('orders_filled', 0):,}")
    with col4:
        st.metric("Total Time", f"{data.phase_times.get('total_secs', 0):.3f}s")

    st.divider()

    st.info(
        """
        **Navigate using the sidebar pages:**
        - **Overview**: Convergence charts, phase timing breakdown
        - **Markets**: Select a market to see price evolution and orders
        - **Orders**: Select an order to see fill history and details
        """
    )


if __name__ == "__main__":
    main()
