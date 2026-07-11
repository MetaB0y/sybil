from enum import Enum

class SystemEventResponseType6Type(str, Enum):
    L1_BLOCK_OBSERVED = "l1_block_observed"

    def __str__(self) -> str:
        return str(self.value)
