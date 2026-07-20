from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.register_key_request import RegisterKeyRequest





T = TypeVar("T", bound="CreateAccountRequest")



@_attrs_define
class CreateAccountRequest:
    """ 
        Attributes:
            initial_balance_nanos (str): Initial account balance. Integer nanodollars; 1_000_000_000 = $1. Example:
                100000000000.
            provisioning_key (str): Caller-stable retry identity. The server binds it to the current
                genesis and exact creation parameters.
            initial_key (None | RegisterKeyRequest | Unset):
     """

    initial_balance_nanos: str
    provisioning_key: str
    initial_key: None | RegisterKeyRequest | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.register_key_request import RegisterKeyRequest
        initial_balance_nanos = self.initial_balance_nanos

        provisioning_key = self.provisioning_key

        initial_key: dict[str, Any] | None | Unset
        if isinstance(self.initial_key, Unset):
            initial_key = UNSET
        elif isinstance(self.initial_key, RegisterKeyRequest):
            initial_key = self.initial_key.to_dict()
        else:
            initial_key = self.initial_key


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "initial_balance_nanos": initial_balance_nanos,
            "provisioning_key": provisioning_key,
        })
        if initial_key is not UNSET:
            field_dict["initial_key"] = initial_key

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.register_key_request import RegisterKeyRequest
        d = dict(src_dict)
        initial_balance_nanos = d.pop("initial_balance_nanos")

        provisioning_key = d.pop("provisioning_key")

        def _parse_initial_key(data: object) -> None | RegisterKeyRequest | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                initial_key_type_1 = RegisterKeyRequest.from_dict(data)



                return initial_key_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RegisterKeyRequest | Unset, data)

        initial_key = _parse_initial_key(d.pop("initial_key", UNSET))


        create_account_request = cls(
            initial_balance_nanos=initial_balance_nanos,
            provisioning_key=provisioning_key,
            initial_key=initial_key,
        )


        create_account_request.additional_properties = d
        return create_account_request

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
