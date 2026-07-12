from enum import Enum

class AutoResolutionActionDto(str, Enum):
    ESCALATE = "escalate"
    PROPOSE = "propose"
    REVIEW = "review"

    def __str__(self) -> str:
        return str(self.value)
