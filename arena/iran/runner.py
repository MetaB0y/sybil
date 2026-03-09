# Moved to sim/runner.py
from sim.runner import (  # noqa: F401
    SimulationConfig,
    TraderSpec,
    main,
    run_simulation,
)
from sim.results import (  # noqa: F401
    build_block_records,
    save_and_print_results,
)

if __name__ == "__main__":
    main()
