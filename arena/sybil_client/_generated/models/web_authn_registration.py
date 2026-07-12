from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="WebAuthnRegistration")



@_attrs_define
class WebAuthnRegistration:
    """ 
        Attributes:
            attestation_object_b64url (str): Base64url-encoded WebAuthn attestationObject from
                `navigator.credentials.create`.
            client_data_json_b64url (str): Base64url-encoded WebAuthn clientDataJSON from `navigator.credentials.create`.
     """

    attestation_object_b64url: str
    client_data_json_b64url: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        attestation_object_b64url = self.attestation_object_b64url

        client_data_json_b64url = self.client_data_json_b64url


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "attestation_object_b64url": attestation_object_b64url,
            "client_data_json_b64url": client_data_json_b64url,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        attestation_object_b64url = d.pop("attestation_object_b64url")

        client_data_json_b64url = d.pop("client_data_json_b64url")

        web_authn_registration = cls(
            attestation_object_b64url=attestation_object_b64url,
            client_data_json_b64url=client_data_json_b64url,
        )


        web_authn_registration.additional_properties = d
        return web_authn_registration

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
