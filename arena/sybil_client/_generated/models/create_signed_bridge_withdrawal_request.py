from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.create_bridge_withdrawal_request import CreateBridgeWithdrawalRequest





T = TypeVar("T", bound="CreateSignedBridgeWithdrawalRequest")



@_attrs_define
class CreateSignedBridgeWithdrawalRequest:
    """ 
        Attributes:
            signature_hex (str): Hex-encoded P256 ECDSA signature over the canonical withdrawal payload.
            signer_pubkey_hex (str): Hex-encoded compressed P256 public key of the signer.
            withdrawal (CreateBridgeWithdrawalRequest):
     """

    signature_hex: str
    signer_pubkey_hex: str
    withdrawal: CreateBridgeWithdrawalRequest
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.create_bridge_withdrawal_request import CreateBridgeWithdrawalRequest
        signature_hex = self.signature_hex

        signer_pubkey_hex = self.signer_pubkey_hex

        withdrawal = self.withdrawal.to_dict()


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "signature_hex": signature_hex,
            "signer_pubkey_hex": signer_pubkey_hex,
            "withdrawal": withdrawal,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.create_bridge_withdrawal_request import CreateBridgeWithdrawalRequest
        d = dict(src_dict)
        signature_hex = d.pop("signature_hex")

        signer_pubkey_hex = d.pop("signer_pubkey_hex")

        withdrawal = CreateBridgeWithdrawalRequest.from_dict(d.pop("withdrawal"))




        create_signed_bridge_withdrawal_request = cls(
            signature_hex=signature_hex,
            signer_pubkey_hex=signer_pubkey_hex,
            withdrawal=withdrawal,
        )


        create_signed_bridge_withdrawal_request.additional_properties = d
        return create_signed_bridge_withdrawal_request

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
