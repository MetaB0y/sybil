from enum import Enum

class ActorRole(str, Enum):
    MARKET_MAKER = "market_maker"
    NOISE = "noise"

    def __str__(self) -> str:
        return str(self.value)
