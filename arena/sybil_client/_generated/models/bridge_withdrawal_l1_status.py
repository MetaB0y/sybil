from enum import Enum

class BridgeWithdrawalL1Status(str, Enum):
    CANCELLED = "cancelled"
    FINALIZED = "finalized"
    NOT_REQUESTED = "not_requested"
    QUEUED = "queued"
    REFUNDED = "refunded"

    def __str__(self) -> str:
        return str(self.value)
