from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.auth_scheme import AuthScheme
from ..models.time_in_force import TimeInForce
from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.signed_order_data import SignedOrderData
  from ..models.web_authn_assertion import WebAuthnAssertion





T = TypeVar("T", bound="SubmitSignedOrderRequest")



@_attrs_define
class SubmitSignedOrderRequest:
    """ 
        Attributes:
            nonce (int): Per-account replay nonce covered by the P256 signature.
            order (SignedOrderData):
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the signer.
            auth_scheme (AuthScheme | Unset):
            expires_at_block (int | None | Unset): Last eligible block height, covered by the P256 signature. Required for
                signed IOC/GTD.
            signature_hex (None | str | Unset): Hex-encoded raw P256 ECDSA signature over the canonical order payload.
                Required when `auth_scheme` is `raw_p256`.
            time_in_force (TimeInForce | Unset):
            webauthn_assertion (None | Unset | WebAuthnAssertion):
     """

    nonce: int
    order: SignedOrderData
    signer_pubkey_hex: str
    auth_scheme: AuthScheme | Unset = UNSET
    expires_at_block: int | None | Unset = UNSET
    signature_hex: None | str | Unset = UNSET
    time_in_force: TimeInForce | Unset = UNSET
    webauthn_assertion: None | Unset | WebAuthnAssertion = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.signed_order_data import SignedOrderData
        from ..models.web_authn_assertion import WebAuthnAssertion
        nonce = self.nonce

        order = self.order.to_dict()

        signer_pubkey_hex = self.signer_pubkey_hex

        auth_scheme: str | Unset = UNSET
        if not isinstance(self.auth_scheme, Unset):
            auth_scheme = self.auth_scheme.value


        expires_at_block: int | None | Unset
        if isinstance(self.expires_at_block, Unset):
            expires_at_block = UNSET
        else:
            expires_at_block = self.expires_at_block

        signature_hex: None | str | Unset
        if isinstance(self.signature_hex, Unset):
            signature_hex = UNSET
        else:
            signature_hex = self.signature_hex

        time_in_force: str | Unset = UNSET
        if not isinstance(self.time_in_force, Unset):
            time_in_force = self.time_in_force.value


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
            "nonce": nonce,
            "order": order,
            "signer_pubkey_hex": signer_pubkey_hex,
        })
        if auth_scheme is not UNSET:
            field_dict["auth_scheme"] = auth_scheme
        if expires_at_block is not UNSET:
            field_dict["expires_at_block"] = expires_at_block
        if signature_hex is not UNSET:
            field_dict["signature_hex"] = signature_hex
        if time_in_force is not UNSET:
            field_dict["time_in_force"] = time_in_force
        if webauthn_assertion is not UNSET:
            field_dict["webauthn_assertion"] = webauthn_assertion

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.signed_order_data import SignedOrderData
        from ..models.web_authn_assertion import WebAuthnAssertion
        d = dict(src_dict)
        nonce = d.pop("nonce")

        order = SignedOrderData.from_dict(d.pop("order"))




        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        _auth_scheme = d.pop("auth_scheme", UNSET)
        auth_scheme: AuthScheme | Unset
        if isinstance(_auth_scheme,  Unset):
            auth_scheme = UNSET
        else:
            auth_scheme = AuthScheme(_auth_scheme)




        def _parse_expires_at_block(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expires_at_block = _parse_expires_at_block(d.pop("expires_at_block", UNSET))


        def _parse_signature_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        signature_hex = _parse_signature_hex(d.pop("signature_hex", UNSET))


        _time_in_force = d.pop("time_in_force", UNSET)
        time_in_force: TimeInForce | Unset
        if isinstance(_time_in_force,  Unset):
            time_in_force = UNSET
        else:
            time_in_force = TimeInForce(_time_in_force)




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


        submit_signed_order_request = cls(
            nonce=nonce,
            order=order,
            signer_pubkey_hex=signer_pubkey_hex,
            auth_scheme=auth_scheme,
            expires_at_block=expires_at_block,
            signature_hex=signature_hex,
            time_in_force=time_in_force,
            webauthn_assertion=webauthn_assertion,
        )


        submit_signed_order_request.additional_properties = d
        return submit_signed_order_request

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
