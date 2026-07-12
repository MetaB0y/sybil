from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="KeyOpStateResponse")



@_attrs_define
class KeyOpStateResponse:
    """ Public, non-secret state needed to construct a one-shot key operation.

        Attributes:
            account_id (int):
            events_digest_hex (str):
            keys_digest_hex (str):
     """

    account_id: int
    events_digest_hex: str
    keys_digest_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        events_digest_hex = self.events_digest_hex

        keys_digest_hex = self.keys_digest_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "events_digest_hex": events_digest_hex,
            "keys_digest_hex": keys_digest_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        events_digest_hex = d.pop("events_digest_hex")

        keys_digest_hex = d.pop("keys_digest_hex")

        key_op_state_response = cls(
            account_id=account_id,
            events_digest_hex=events_digest_hex,
            keys_digest_hex=keys_digest_hex,
        )


        key_op_state_response.additional_properties = d
        return key_op_state_response

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
