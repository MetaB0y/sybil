from enum import Enum

class SystemEventResponseType11Type(str, Enum):
    KEY_REVOKED = "key_revoked"

    def __str__(self) -> str:
        return str(self.value)
