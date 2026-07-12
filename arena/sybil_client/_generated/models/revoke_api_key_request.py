from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.auth_scheme import AuthScheme
from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.web_authn_assertion import WebAuthnAssertion





T = TypeVar("T", bound="RevokeApiKeyRequest")



@_attrs_define
class RevokeApiKeyRequest:
    """ Signed request to revoke a read-scoped bearer API key by id (SYB-60).

        Attributes:
            api_key_id (int): Id of the API key to revoke (from the API-key listing).
            nonce (int):
            signer_pubkey_hex (str):
            auth_scheme (AuthScheme | Unset):
            signature_hex (None | str | Unset):
            webauthn_assertion (None | Unset | WebAuthnAssertion):
     """

    api_key_id: int
    nonce: int
    signer_pubkey_hex: str
    auth_scheme: AuthScheme | Unset = UNSET
    signature_hex: None | str | Unset = UNSET
    webauthn_assertion: None | Unset | WebAuthnAssertion = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.web_authn_assertion import WebAuthnAssertion
        api_key_id = self.api_key_id

        nonce = self.nonce

        signer_pubkey_hex = self.signer_pubkey_hex

        auth_scheme: str | Unset = UNSET
        if not isinstance(self.auth_scheme, Unset):
            auth_scheme = self.auth_scheme.value


        signature_hex: None | str | Unset
        if isinstance(self.signature_hex, Unset):
            signature_hex = UNSET
        else:
            signature_hex = self.signature_hex

        webauthn_assertion: dict[str, Any] | None | Unset
        if isinstance(self.webauthn_assertion, Unset):
            webauthn_assertion = UNSET
        elif isinstance(self.webauthn_assertion, WebAuthnAssertion):
            webauthn_assertion = self.webauthn_assertion.to_dict()
        else:
            webauthn_assertion = self.webauthn_assertion


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "api_key_id": api_key_id,
            "nonce": nonce,
            "signer_pubkey_hex": signer_pubkey_hex,
        })
        if auth_scheme is not UNSET:
            field_dict["auth_scheme"] = auth_scheme
        if signature_hex is not UNSET:
            field_dict["signature_hex"] = signature_hex
        if webauthn_assertion is not UNSET:
            field_dict["webauthn_assertion"] = webauthn_assertion

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.web_authn_assertion import WebAuthnAssertion
        d = dict(src_dict)
        api_key_id = d.pop("api_key_id")

        nonce = d.pop("nonce")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        _auth_scheme = d.pop("auth_scheme", UNSET)
        auth_scheme: AuthScheme | Unset
        if isinstance(_auth_scheme,  Unset):
            auth_scheme = UNSET
        else:
            auth_scheme = AuthScheme(_auth_scheme)




        def _parse_signature_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        signature_hex = _parse_signature_hex(d.pop("signature_hex", UNSET))


        def _parse_webauthn_assertion(data: object) -> None | Unset | WebAuthnAssertion:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                webauthn_assertion_type_1 = WebAuthnAssertion.from_dict(data)



                return webauthn_assertion_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | WebAuthnAssertion, data)

        webauthn_assertion = _parse_webauthn_assertion(d.pop("webauthn_assertion", UNSET))


        revoke_api_key_request = cls(
            api_key_id=api_key_id,
            nonce=nonce,
            signer_pubkey_hex=signer_pubkey_hex,
            auth_scheme=auth_scheme,
            signature_hex=signature_hex,
            webauthn_assertion=webauthn_assertion,
        )


        revoke_api_key_request.additional_properties = d
        return revoke_api_key_request

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
