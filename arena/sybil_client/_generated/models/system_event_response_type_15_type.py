from enum import Enum

class SystemEventResponseType15Type(str, Enum):
    COMPLETE_SET_COLLATERALIZED = "complete_set_collateralized"

    def __str__(self) -> str:
        return str(self.value)
