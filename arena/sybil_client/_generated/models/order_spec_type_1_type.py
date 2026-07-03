from enum import Enum

class OrderSpecType1Type(str, Enum):
    BUYNO = "BuyNo"

    def __str__(self) -> str:
        return str(self.value)
