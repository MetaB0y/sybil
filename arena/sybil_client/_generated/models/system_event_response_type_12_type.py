from enum import Enum

class SystemEventResponseType12Type(str, Enum):
    DEPOSIT_QUARANTINED = "deposit_quarantined"

    def __str__(self) -> str:
        return str(self.value)
