from enum import Enum

class KeyScope(str, Enum):
    AGENT = "agent"
    CUSTOM = "custom"
    PRIMARY = "primary"

    def __str__(self) -> str:
        return str(self.value)
