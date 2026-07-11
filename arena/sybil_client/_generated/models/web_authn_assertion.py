from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="WebAuthnAssertion")



@_attrs_define
class WebAuthnAssertion:
    """ 
        Attributes:
            authenticator_data_b64url (str): Base64url authenticatorData bytes from `navigator.credentials.get`.
            client_data_json_b64url (str): Base64url clientDataJSON bytes from `navigator.credentials.get`.
            credential_id_b64url (str): Base64url credential id returned by the authenticator.
            signature_b64url (str): Base64url DER-encoded ECDSA signature from `navigator.credentials.get`.
            user_handle_b64url (None | str | Unset): Optional base64url userHandle returned by the authenticator.
     """

    authenticator_data_b64url: str
    client_data_json_b64url: str
    credential_id_b64url: str
    signature_b64url: str
    user_handle_b64url: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        authenticator_data_b64url = self.authenticator_data_b64url

        client_data_json_b64url = self.client_data_json_b64url

        credential_id_b64url = self.credential_id_b64url

        signature_b64url = self.signature_b64url

        user_handle_b64url: None | str | Unset
        if isinstance(self.user_handle_b64url, Unset):
            user_handle_b64url = UNSET
        else:
            user_handle_b64url = self.user_handle_b64url


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "authenticator_data_b64url": authenticator_data_b64url,
            "client_data_json_b64url": client_data_json_b64url,
            "credential_id_b64url": credential_id_b64url,
            "signature_b64url": signature_b64url,
        })
        if user_handle_b64url is not UNSET:
            field_dict["user_handle_b64url"] = user_handle_b64url

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        authenticator_data_b64url = d.pop("authenticator_data_b64url")

        client_data_json_b64url = d.pop("client_data_json_b64url")

        credential_id_b64url = d.pop("credential_id_b64url")

        signature_b64url = d.pop("signature_b64url")

        def _parse_user_handle_b64url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        user_handle_b64url = _parse_user_handle_b64url(d.pop("user_handle_b64url", UNSET))


        web_authn_assertion = cls(
            authenticator_data_b64url=authenticator_data_b64url,
            client_data_json_b64url=client_data_json_b64url,
            credential_id_b64url=credential_id_b64url,
            signature_b64url=signature_b64url,
            user_handle_b64url=user_handle_b64url,
        )


        web_authn_assertion.additional_properties = d
        return web_authn_assertion

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
