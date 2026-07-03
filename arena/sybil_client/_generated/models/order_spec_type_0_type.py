from enum import Enum

class OrderSpecType0Type(str, Enum):
    BUYYES = "BuyYes"

    def __str__(self) -> str:
        return str(self.value)
