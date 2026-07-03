from enum import Enum

class SystemEventResponseType1Type(str, Enum):
    DEPOSIT = "deposit"

    def __str__(self) -> str:
        return str(self.value)
