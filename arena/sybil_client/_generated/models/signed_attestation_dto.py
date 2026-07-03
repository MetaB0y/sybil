from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="SignedAttestationDto")



@_attrs_define
class SignedAttestationDto:
    """ Wire form of a signed resolution attestation.

        Attributes:
            nonce (int): Nonce the signer chose (typically `timestamp_ms`).
            pubkey_hex (str): Hex-encoded compressed SEC1 P256 public key (33 bytes).
            signature_hex (str): Hex-encoded P256 ECDSA signature over the canonical attestation bytes.
     """

    nonce: int
    pubkey_hex: str
    signature_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        nonce = self.nonce

        pubkey_hex = self.pubkey_hex

        signature_hex = self.signature_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "nonce": nonce,
            "pubkey_hex": pubkey_hex,
            "signature_hex": signature_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        nonce = d.pop("nonce")

        pubkey_hex = d.pop("pubkey_hex")

        signature_hex = d.pop("signature_hex")

        signed_attestation_dto = cls(
            nonce=nonce,
            pubkey_hex=pubkey_hex,
            signature_hex=signature_hex,
        )


        signed_attestation_dto.additional_properties = d
        return signed_attestation_dto

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
