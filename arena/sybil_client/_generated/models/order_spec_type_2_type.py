from enum import Enum

class OrderSpecType2Type(str, Enum):
    SELLYES = "SellYes"

    def __str__(self) -> str:
        return str(self.value)
