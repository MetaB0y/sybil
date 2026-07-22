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
  from ..models.order_spec_type_0 import OrderSpecType0
  from ..models.order_spec_type_1 import OrderSpecType1
  from ..models.order_spec_type_2 import OrderSpecType2
  from ..models.order_spec_type_3 import OrderSpecType3
  from ..models.web_authn_assertion import WebAuthnAssertion





T = TypeVar("T", bound="SubmitSignedMmBundleRequest")



@_attrs_define
class SubmitSignedMmBundleRequest:
    """ Public signed submission of one all-or-nothing MM quote bundle.

        Attributes:
            account_id (int): Account that owns the bundle. It must match the signer's registration.
            bundle_id_hex (str): Client-chosen opaque 32-byte bundle identity, hex encoded.
            expires_at_block (int): Exact next block this IOC bundle targets. The actor rejects any other height.
            mm_budget_nanos (str): Integer nanodollars: one flash-liquidity budget shared by every quote in the bundle.
            nonce (int): Per-account replay nonce covered by the signature.
            orders (list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3]): Every quote in the atomic
                bundle. All quote fields and their order are signed.
            revision (int): Initial submissions use revision zero.
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the signer.
            auth_scheme (AuthScheme | Unset):
            signature_hex (None | str | Unset): Hex-encoded raw P256 ECDSA signature over canonical bundle bytes.
            webauthn_assertion (None | Unset | WebAuthnAssertion):
     """

    account_id: int
    bundle_id_hex: str
    expires_at_block: int
    mm_budget_nanos: str
    nonce: int
    orders: list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3]
    revision: int
    signer_pubkey_hex: str
    auth_scheme: AuthScheme | Unset = UNSET
    signature_hex: None | str | Unset = UNSET
    webauthn_assertion: None | Unset | WebAuthnAssertion = UNSET





    def to_dict(self) -> dict[str, Any]:
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        from ..models.web_authn_assertion import WebAuthnAssertion
        account_id = self.account_id

        bundle_id_hex = self.bundle_id_hex

        expires_at_block = self.expires_at_block

        mm_budget_nanos = self.mm_budget_nanos

        nonce = self.nonce

        orders = []
        for orders_item_data in self.orders:
            orders_item: dict[str, Any]
            if isinstance(orders_item_data, OrderSpecType0):
                orders_item = orders_item_data.to_dict()
            elif isinstance(orders_item_data, OrderSpecType1):
                orders_item = orders_item_data.to_dict()
            elif isinstance(orders_item_data, OrderSpecType2):
                orders_item = orders_item_data.to_dict()
            else:
                orders_item = orders_item_data.to_dict()

            orders.append(orders_item)



        revision = self.revision

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

        field_dict.update({
            "account_id": account_id,
            "bundle_id_hex": bundle_id_hex,
            "expires_at_block": expires_at_block,
            "mm_budget_nanos": mm_budget_nanos,
            "nonce": nonce,
            "orders": orders,
            "revision": revision,
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
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        from ..models.web_authn_assertion import WebAuthnAssertion
        d = dict(src_dict)
        account_id = d.pop("account_id")

        bundle_id_hex = d.pop("bundle_id_hex")

        expires_at_block = d.pop("expires_at_block")

        mm_budget_nanos = d.pop("mm_budget_nanos")

        nonce = d.pop("nonce")

        orders = []
        _orders = d.pop("orders")
        for orders_item_data in (_orders):
            def _parse_orders_item(data: object) -> OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3:
                try:
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_order_spec_type_0 = OrderSpecType0.from_dict(data)



                    return componentsschemas_order_spec_type_0
                except (TypeError, ValueError, AttributeError, KeyError):
                    pass
                try:
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_order_spec_type_1 = OrderSpecType1.from_dict(data)



                    return componentsschemas_order_spec_type_1
                except (TypeError, ValueError, AttributeError, KeyError):
                    pass
                try:
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_order_spec_type_2 = OrderSpecType2.from_dict(data)



                    return componentsschemas_order_spec_type_2
                except (TypeError, ValueError, AttributeError, KeyError):
                    pass
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_order_spec_type_3 = OrderSpecType3.from_dict(data)



                return componentsschemas_order_spec_type_3

            orders_item = _parse_orders_item(orders_item_data)

            orders.append(orders_item)


        revision = d.pop("revision")

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


        submit_signed_mm_bundle_request = cls(
            account_id=account_id,
            bundle_id_hex=bundle_id_hex,
            expires_at_block=expires_at_block,
            mm_budget_nanos=mm_budget_nanos,
            nonce=nonce,
            orders=orders,
            revision=revision,
            signer_pubkey_hex=signer_pubkey_hex,
            auth_scheme=auth_scheme,
            signature_hex=signature_hex,
            webauthn_assertion=webauthn_assertion,
        )

        return submit_signed_mm_bundle_request

