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
  from ..models.web_authn_assertion import WebAuthnAssertion
  from ..models.web_authn_registration import WebAuthnRegistration





T = TypeVar("T", bound="SignedRegisterKeyRequest")



@_attrs_define
class SignedRegisterKeyRequest:
    """ Signed request to register a NEW signing key on an account (SYB-229).

    Required whenever the account already has at least one registered key. The
    first key is bootstrapped over the service tier (`POST /v1/accounts/{id}/keys`);
    every subsequent key must be authorized by a signature from an existing
    account key. Like orders/cancels, the canonical payload is domain-separated
    by the chain `genesis_hash` (SYB-224).

        Attributes:
            bound_events_digest_hex (str): Hex account event-chain digest the authorization is state-bound to.
            bound_keys_digest_hex (str): Hex account key-set digest the authorization is state-bound to.
            public_key_hex (str): Hex-encoded compressed P256 public key (33 bytes) of the NEW key. Example:
                036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296.
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the SIGNER — an existing active
                key on this account authorizing the registration.
            auth_scheme (AuthScheme | Unset):
            credential_id_b64url (None | str | Unset): Base64url credential id for a WebAuthn new key.
            label (None | str | Unset): Optional human label for the new key, e.g. "agent:pricer", limited to
                128 UTF-8 bytes.
            scope (KeyScope | Unset): Scope tag for a registered signing key (SYB-60).
            signature_hex (None | str | Unset): Hex-encoded raw P256 ECDSA signature over the canonical registration
                payload. Required when `signer_auth_scheme` is `raw_p256`.
            signer_auth_scheme (AuthScheme | Unset):
            webauthn_assertion (None | Unset | WebAuthnAssertion):
            webauthn_registration (None | Unset | WebAuthnRegistration):
     """

    bound_events_digest_hex: str
    bound_keys_digest_hex: str
    public_key_hex: str
    signer_pubkey_hex: str
    auth_scheme: AuthScheme | Unset = UNSET
    credential_id_b64url: None | str | Unset = UNSET
    label: None | str | Unset = UNSET
    scope: KeyScope | Unset = UNSET
    signature_hex: None | str | Unset = UNSET
    signer_auth_scheme: AuthScheme | Unset = UNSET
    webauthn_assertion: None | Unset | WebAuthnAssertion = UNSET
    webauthn_registration: None | Unset | WebAuthnRegistration = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.web_authn_assertion import WebAuthnAssertion
        from ..models.web_authn_registration import WebAuthnRegistration
        bound_events_digest_hex = self.bound_events_digest_hex

        bound_keys_digest_hex = self.bound_keys_digest_hex

        public_key_hex = self.public_key_hex

        signer_pubkey_hex = self.signer_pubkey_hex

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


        signature_hex: None | str | Unset
        if isinstance(self.signature_hex, Unset):
            signature_hex = UNSET
        else:
            signature_hex = self.signature_hex

        signer_auth_scheme: str | Unset = UNSET
        if not isinstance(self.signer_auth_scheme, Unset):
            signer_auth_scheme = self.signer_auth_scheme.value


        webauthn_assertion: dict[str, Any] | None | Unset
        if isinstance(self.webauthn_assertion, Unset):
            webauthn_assertion = UNSET
        elif isinstance(self.webauthn_assertion, WebAuthnAssertion):
            webauthn_assertion = self.webauthn_assertion.to_dict()
        else:
            webauthn_assertion = self.webauthn_assertion

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
            "bound_events_digest_hex": bound_events_digest_hex,
            "bound_keys_digest_hex": bound_keys_digest_hex,
            "public_key_hex": public_key_hex,
            "signer_pubkey_hex": signer_pubkey_hex,
        })
        if auth_scheme is not UNSET:
            field_dict["auth_scheme"] = auth_scheme
        if credential_id_b64url is not UNSET:
            field_dict["credential_id_b64url"] = credential_id_b64url
        if label is not UNSET:
            field_dict["label"] = label
        if scope is not UNSET:
            field_dict["scope"] = scope
        if signature_hex is not UNSET:
            field_dict["signature_hex"] = signature_hex
        if signer_auth_scheme is not UNSET:
            field_dict["signer_auth_scheme"] = signer_auth_scheme
        if webauthn_assertion is not UNSET:
            field_dict["webauthn_assertion"] = webauthn_assertion
        if webauthn_registration is not UNSET:
            field_dict["webauthn_registration"] = webauthn_registration

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.web_authn_assertion import WebAuthnAssertion
        from ..models.web_authn_registration import WebAuthnRegistration
        d = dict(src_dict)
        bound_events_digest_hex = d.pop("bound_events_digest_hex")

        bound_keys_digest_hex = d.pop("bound_keys_digest_hex")

        public_key_hex = d.pop("public_key_hex")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

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




        def _parse_signature_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        signature_hex = _parse_signature_hex(d.pop("signature_hex", UNSET))


        _signer_auth_scheme = d.pop("signer_auth_scheme", UNSET)
        signer_auth_scheme: AuthScheme | Unset
        if isinstance(_signer_auth_scheme,  Unset):
            signer_auth_scheme = UNSET
        else:
            signer_auth_scheme = AuthScheme(_signer_auth_scheme)




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


        signed_register_key_request = cls(
            bound_events_digest_hex=bound_events_digest_hex,
            bound_keys_digest_hex=bound_keys_digest_hex,
            public_key_hex=public_key_hex,
            signer_pubkey_hex=signer_pubkey_hex,
            auth_scheme=auth_scheme,
            credential_id_b64url=credential_id_b64url,
            label=label,
            scope=scope,
            signature_hex=signature_hex,
            signer_auth_scheme=signer_auth_scheme,
            webauthn_assertion=webauthn_assertion,
            webauthn_registration=webauthn_registration,
        )


        signed_register_key_request.additional_properties = d
        return signed_register_key_request

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
