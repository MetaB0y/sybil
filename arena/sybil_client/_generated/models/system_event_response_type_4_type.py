from enum import Enum

class SystemEventResponseType4Type(str, Enum):
    WITHDRAWAL_REFUNDED = "withdrawal_refunded"

    def __str__(self) -> str:
        return str(self.value)
