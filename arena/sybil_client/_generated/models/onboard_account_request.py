from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.register_key_request import RegisterKeyRequest





T = TypeVar("T", bound="OnboardAccountRequest")



@_attrs_define
class OnboardAccountRequest:
    """ Public self-service account onboarding.

    The server, not the caller, chooses the play-money grant. Keeping funding
    out of this DTO prevents anonymous callers from turning account allocation
    into an arbitrary minting interface.

        Attributes:
            initial_key (RegisterKeyRequest):
     """

    initial_key: RegisterKeyRequest





    def to_dict(self) -> dict[str, Any]:
        from ..models.register_key_request import RegisterKeyRequest
        initial_key = self.initial_key.to_dict()


        field_dict: dict[str, Any] = {}

        field_dict.update({
            "initial_key": initial_key,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.register_key_request import RegisterKeyRequest
        d = dict(src_dict)
        initial_key = RegisterKeyRequest.from_dict(d.pop("initial_key"))




        onboard_account_request = cls(
            initial_key=initial_key,
        )

        return onboard_account_request

