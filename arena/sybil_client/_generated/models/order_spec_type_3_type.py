from enum import Enum

class OrderSpecType3Type(str, Enum):
    SELLNO = "SellNo"

    def __str__(self) -> str:
        return str(self.value)
