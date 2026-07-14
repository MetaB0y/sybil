from enum import Enum

class SystemEventResponseType17Type(str, Enum):
    LIQUIDITY_UNIVERSE_ACTIVATED = "liquidity_universe_activated"

    def __str__(self) -> str:
        return str(self.value)
