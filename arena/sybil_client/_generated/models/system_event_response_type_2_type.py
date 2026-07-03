from enum import Enum

class SystemEventResponseType2Type(str, Enum):
    L1_DEPOSIT = "l1_deposit"

    def __str__(self) -> str:
        return str(self.value)
