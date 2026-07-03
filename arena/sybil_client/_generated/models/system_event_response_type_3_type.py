from enum import Enum

class SystemEventResponseType3Type(str, Enum):
    WITHDRAWAL_CREATED = "withdrawal_created"

    def __str__(self) -> str:
        return str(self.value)
