from enum import Enum

class SystemEventResponseType4Type(str, Enum):
    MARKET_RESOLVED = "market_resolved"

    def __str__(self) -> str:
        return str(self.value)
