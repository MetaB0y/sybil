from enum import Enum

class CompleteSetActionRequestType1Action(str, Enum):
    REDEEM = "redeem"

    def __str__(self) -> str:
        return str(self.value)
