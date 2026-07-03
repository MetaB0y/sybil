from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="CancelSignedOrderRequest")



@_attrs_define
class CancelSignedOrderRequest:
    """ 
        Attributes:
            account_id (int): Account ID claiming ownership of the order being cancelled.
            order_id (int): The pending order to cancel.
            signature_hex (str): Hex-encoded P256 ECDSA signature over the canonical cancel payload.
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the signer.
     """

    account_id: int
    order_id: int
    signature_hex: str
    signer_pubkey_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        order_id = self.order_id

        signature_hex = self.signature_hex

        signer_pubkey_hex = self.signer_pubkey_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "order_id": order_id,
            "signature_hex": signature_hex,
            "signer_pubkey_hex": signer_pubkey_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        order_id = d.pop("order_id")

        signature_hex = d.pop("signature_hex")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        cancel_signed_order_request = cls(
            account_id=account_id,
            order_id=order_id,
            signature_hex=signature_hex,
            signer_pubkey_hex=signer_pubkey_hex,
        )


        cancel_signed_order_request.additional_properties = d
        return cancel_signed_order_request

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
