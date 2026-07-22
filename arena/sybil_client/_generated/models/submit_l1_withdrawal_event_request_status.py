from enum import Enum

class SubmitL1WithdrawalEventRequestStatus(str, Enum):
    CANCELLED = "cancelled"
    FINALIZED = "finalized"
    NOT_REQUESTED = "not_requested"
    QUEUED = "queued"

    def __str__(self) -> str:
        return str(self.value)
