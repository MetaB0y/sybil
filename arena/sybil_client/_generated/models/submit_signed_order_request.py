from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.time_in_force import TimeInForce
from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.signed_order_data import SignedOrderData





T = TypeVar("T", bound="SubmitSignedOrderRequest")



@_attrs_define
class SubmitSignedOrderRequest:
    """ 
        Attributes:
            nonce (int): Per-account replay nonce covered by the P256 signature.
            order (SignedOrderData):
            signature_hex (str): Hex-encoded P256 ECDSA signature.
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the signer.
            expires_at_block (int | None | Unset): Last eligible block height, covered by the P256 signature. Required for
                signed IOC/GTD.
            time_in_force (TimeInForce | Unset):
     """

    nonce: int
    order: SignedOrderData
    signature_hex: str
    signer_pubkey_hex: str
    expires_at_block: int | None | Unset = UNSET
    time_in_force: TimeInForce | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.signed_order_data import SignedOrderData
        nonce = self.nonce

        order = self.order.to_dict()

        signature_hex = self.signature_hex

        signer_pubkey_hex = self.signer_pubkey_hex

        expires_at_block: int | None | Unset
        if isinstance(self.expires_at_block, Unset):
            expires_at_block = UNSET
        else:
            expires_at_block = self.expires_at_block

        time_in_force: str | Unset = UNSET
        if not isinstance(self.time_in_force, Unset):
            time_in_force = self.time_in_force.value



        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "nonce": nonce,
            "order": order,
            "signature_hex": signature_hex,
            "signer_pubkey_hex": signer_pubkey_hex,
        })
        if expires_at_block is not UNSET:
            field_dict["expires_at_block"] = expires_at_block
        if time_in_force is not UNSET:
            field_dict["time_in_force"] = time_in_force

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.signed_order_data import SignedOrderData
        d = dict(src_dict)
        nonce = d.pop("nonce")

        order = SignedOrderData.from_dict(d.pop("order"))




        signature_hex = d.pop("signature_hex")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        def _parse_expires_at_block(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expires_at_block = _parse_expires_at_block(d.pop("expires_at_block", UNSET))


        _time_in_force = d.pop("time_in_force", UNSET)
        time_in_force: TimeInForce | Unset
        if isinstance(_time_in_force,  Unset):
            time_in_force = UNSET
        else:
            time_in_force = TimeInForce(_time_in_force)




        submit_signed_order_request = cls(
            nonce=nonce,
            order=order,
            signature_hex=signature_hex,
            signer_pubkey_hex=signer_pubkey_hex,
            expires_at_block=expires_at_block,
            time_in_force=time_in_force,
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
