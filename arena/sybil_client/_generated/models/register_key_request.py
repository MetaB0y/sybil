from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.auth_scheme import AuthScheme
from ..models.key_scope import KeyScope
from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.web_authn_registration import WebAuthnRegistration





T = TypeVar("T", bound="RegisterKeyRequest")



@_attrs_define
class RegisterKeyRequest:
    """ 
        Attributes:
            public_key_hex (str): Hex-encoded compressed P256 public key (33 bytes). Example:
                036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296.
            auth_scheme (AuthScheme | Unset):
            credential_id_b64url (None | str | Unset): Base64url credential id for WebAuthn keys. Stored client-side today
                and
                documented here so passkey clients can round-trip the registration payload.
            label (None | str | Unset): Optional human label for this key, e.g. "agent:pricer" (SYB-60),
                limited to 128 UTF-8 bytes.
            scope (KeyScope | Unset): Scope tag for a registered signing key (SYB-60).
            webauthn_registration (None | Unset | WebAuthnRegistration):
     """

    public_key_hex: str
    auth_scheme: AuthScheme | Unset = UNSET
    credential_id_b64url: None | str | Unset = UNSET
    label: None | str | Unset = UNSET
    scope: KeyScope | Unset = UNSET
    webauthn_registration: None | Unset | WebAuthnRegistration = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.web_authn_registration import WebAuthnRegistration
        public_key_hex = self.public_key_hex

        auth_scheme: str | Unset = UNSET
        if not isinstance(self.auth_scheme, Unset):
            auth_scheme = self.auth_scheme.value


        credential_id_b64url: None | str | Unset
        if isinstance(self.credential_id_b64url, Unset):
            credential_id_b64url = UNSET
        else:
            credential_id_b64url = self.credential_id_b64url

        label: None | str | Unset
        if isinstance(self.label, Unset):
            label = UNSET
        else:
            label = self.label

        scope: str | Unset = UNSET
        if not isinstance(self.scope, Unset):
            scope = self.scope.value


        webauthn_registration: dict[str, Any] | None | Unset
        if isinstance(self.webauthn_registration, Unset):
            webauthn_registration = UNSET
        elif isinstance(self.webauthn_registration, WebAuthnRegistration):
            webauthn_registration = self.webauthn_registration.to_dict()
        else:
            webauthn_registration = self.webauthn_registration


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "public_key_hex": public_key_hex,
        })
        if auth_scheme is not UNSET:
            field_dict["auth_scheme"] = auth_scheme
        if credential_id_b64url is not UNSET:
            field_dict["credential_id_b64url"] = credential_id_b64url
        if label is not UNSET:
            field_dict["label"] = label
        if scope is not UNSET:
            field_dict["scope"] = scope
        if webauthn_registration is not UNSET:
            field_dict["webauthn_registration"] = webauthn_registration

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.web_authn_registration import WebAuthnRegistration
        d = dict(src_dict)
        public_key_hex = d.pop("public_key_hex")

        _auth_scheme = d.pop("auth_scheme", UNSET)
        auth_scheme: AuthScheme | Unset
        if isinstance(_auth_scheme,  Unset):
            auth_scheme = UNSET
        else:
            auth_scheme = AuthScheme(_auth_scheme)




        def _parse_credential_id_b64url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        credential_id_b64url = _parse_credential_id_b64url(d.pop("credential_id_b64url", UNSET))


        def _parse_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        label = _parse_label(d.pop("label", UNSET))


        _scope = d.pop("scope", UNSET)
        scope: KeyScope | Unset
        if isinstance(_scope,  Unset):
            scope = UNSET
        else:
            scope = KeyScope(_scope)




        def _parse_webauthn_registration(data: object) -> None | Unset | WebAuthnRegistration:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                webauthn_registration_type_1 = WebAuthnRegistration.from_dict(data)



                return webauthn_registration_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | WebAuthnRegistration, data)

        webauthn_registration = _parse_webauthn_registration(d.pop("webauthn_registration", UNSET))


        register_key_request = cls(
            public_key_hex=public_key_hex,
            auth_scheme=auth_scheme,
            credential_id_b64url=credential_id_b64url,
            label=label,
            scope=scope,
            webauthn_registration=webauthn_registration,
        )


        register_key_request.additional_properties = d
        return register_key_request

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
