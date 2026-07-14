from enum import Enum

class SystemEventResponseType13Type(str, Enum):
    DEPOSIT_QUARANTINED = "deposit_quarantined"

    def __str__(self) -> str:
        return str(self.value)
