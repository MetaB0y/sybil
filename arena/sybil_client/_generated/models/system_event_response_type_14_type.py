from enum import Enum

class SystemEventResponseType14Type(str, Enum):
    QUARANTINE_CLAIMED = "quarantine_claimed"

    def __str__(self) -> str:
        return str(self.value)
