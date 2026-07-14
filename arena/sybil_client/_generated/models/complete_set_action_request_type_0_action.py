from enum import Enum

class CompleteSetActionRequestType0Action(str, Enum):
    COLLATERALIZE = "collateralize"

    def __str__(self) -> str:
        return str(self.value)
