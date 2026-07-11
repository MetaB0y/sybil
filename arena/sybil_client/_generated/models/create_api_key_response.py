from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="CreateApiKeyResponse")



@_attrs_define
class CreateApiKeyResponse:
    """ Response to creating a read API key (SYB-60). The plaintext `token` is shown
    exactly once here and is not recoverable afterwards.

        Attributes:
            created_at_ms (int):
            id (int):
            signer_pubkey_hex (str): The active signing key that authorized creation. This is especially
                useful during discoverable WebAuthn login, where the browser assertion
                does not itself expose the credential public key.
            token (str): The bearer token. Store it now — the server keeps only its blake3 hash.
            label (None | str | Unset):
     """

    created_at_ms: int
    id: int
    signer_pubkey_hex: str
    token: str
    label: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        created_at_ms = self.created_at_ms

        id = self.id

        signer_pubkey_hex = self.signer_pubkey_hex

        token = self.token

        label: None | str | Unset
        if isinstance(self.label, Unset):
            label = UNSET
        else:
            label = self.label


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "created_at_ms": created_at_ms,
            "id": id,
            "signer_pubkey_hex": signer_pubkey_hex,
            "token": token,
        })
        if label is not UNSET:
            field_dict["label"] = label

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        created_at_ms = d.pop("created_at_ms")

        id = d.pop("id")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        token = d.pop("token")

        def _parse_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        label = _parse_label(d.pop("label", UNSET))


        create_api_key_response = cls(
            created_at_ms=created_at_ms,
            id=id,
            signer_pubkey_hex=signer_pubkey_hex,
            token=token,
            label=label,
        )


        create_api_key_response.additional_properties = d
        return create_api_key_response

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
