from enum import Enum

class SystemEventResponseType9Type(str, Enum):
    MARKET_GROUP_EXTENDED = "market_group_extended"

    def __str__(self) -> str:
        return str(self.value)
