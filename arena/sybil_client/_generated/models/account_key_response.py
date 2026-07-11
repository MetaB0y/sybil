from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="AccountKeyResponse")



@_attrs_define
class AccountKeyResponse:
    """ A registered signing key with SYB-60 management metadata.

        Attributes:
            auth_scheme (str): Authentication scheme: `raw_p256` or `webauthn`.
            created_at_ms (int): Registration time in Unix milliseconds (0 for keys predating SYB-60).
            public_key_hex (str): Hex-encoded compressed P256 public key (33 bytes).
            scope (str): Scope tag: `primary`, `agent`, or `custom`.
            label (None | str | Unset): Optional human label.
     """

    auth_scheme: str
    created_at_ms: int
    public_key_hex: str
    scope: str
    label: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        auth_scheme = self.auth_scheme

        created_at_ms = self.created_at_ms

        public_key_hex = self.public_key_hex

        scope = self.scope

        label: None | str | Unset
        if isinstance(self.label, Unset):
            label = UNSET
        else:
            label = self.label


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "auth_scheme": auth_scheme,
            "created_at_ms": created_at_ms,
            "public_key_hex": public_key_hex,
            "scope": scope,
        })
        if label is not UNSET:
            field_dict["label"] = label

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        auth_scheme = d.pop("auth_scheme")

        created_at_ms = d.pop("created_at_ms")

        public_key_hex = d.pop("public_key_hex")

        scope = d.pop("scope")

        def _parse_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        label = _parse_label(d.pop("label", UNSET))


        account_key_response = cls(
            auth_scheme=auth_scheme,
            created_at_ms=created_at_ms,
            public_key_hex=public_key_hex,
            scope=scope,
            label=label,
        )


        account_key_response.additional_properties = d
        return account_key_response

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
