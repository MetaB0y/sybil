from enum import Enum

class SystemEventResponseType5Type(str, Enum):
    WITHDRAWAL_FINALIZED = "withdrawal_finalized"

    def __str__(self) -> str:
        return str(self.value)
