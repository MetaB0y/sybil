from enum import Enum

class SystemEventResponseType10Type(str, Enum):
    KEY_REGISTERED = "key_registered"

    def __str__(self) -> str:
        return str(self.value)
