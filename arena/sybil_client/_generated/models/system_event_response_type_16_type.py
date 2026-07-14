from enum import Enum

class SystemEventResponseType16Type(str, Enum):
    COMPLETE_SET_REDEEMED = "complete_set_redeemed"

    def __str__(self) -> str:
        return str(self.value)
