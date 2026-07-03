from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="BridgeWithdrawalResponse")



@_attrs_define
class BridgeWithdrawalResponse:
    """ 
        Attributes:
            account_id (int):
            amount_nanos (int):
            amount_token_units (int):
            created_at_height (int):
            expiry_height (int):
            nullifier_hex (str):
            recipient_hex (str):
            token_hex (str):
            withdrawal_id (int):
            withdrawal_leaf_digest_hex (str):
            withdrawal_leaf_hex (str):
     """

    account_id: int
    amount_nanos: int
    amount_token_units: int
    created_at_height: int
    expiry_height: int
    nullifier_hex: str
    recipient_hex: str
    token_hex: str
    withdrawal_id: int
    withdrawal_leaf_digest_hex: str
    withdrawal_leaf_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        amount_nanos = self.amount_nanos

        amount_token_units = self.amount_token_units

        created_at_height = self.created_at_height

        expiry_height = self.expiry_height

        nullifier_hex = self.nullifier_hex

        recipient_hex = self.recipient_hex

        token_hex = self.token_hex

        withdrawal_id = self.withdrawal_id

        withdrawal_leaf_digest_hex = self.withdrawal_leaf_digest_hex

        withdrawal_leaf_hex = self.withdrawal_leaf_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "amount_nanos": amount_nanos,
            "amount_token_units": amount_token_units,
            "created_at_height": created_at_height,
            "expiry_height": expiry_height,
            "nullifier_hex": nullifier_hex,
            "recipient_hex": recipient_hex,
            "token_hex": token_hex,
            "withdrawal_id": withdrawal_id,
            "withdrawal_leaf_digest_hex": withdrawal_leaf_digest_hex,
            "withdrawal_leaf_hex": withdrawal_leaf_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        amount_nanos = d.pop("amount_nanos")

        amount_token_units = d.pop("amount_token_units")

        created_at_height = d.pop("created_at_height")

        expiry_height = d.pop("expiry_height")

        nullifier_hex = d.pop("nullifier_hex")

        recipient_hex = d.pop("recipient_hex")

        token_hex = d.pop("token_hex")

        withdrawal_id = d.pop("withdrawal_id")

        withdrawal_leaf_digest_hex = d.pop("withdrawal_leaf_digest_hex")

        withdrawal_leaf_hex = d.pop("withdrawal_leaf_hex")

        bridge_withdrawal_response = cls(
            account_id=account_id,
            amount_nanos=amount_nanos,
            amount_token_units=amount_token_units,
            created_at_height=created_at_height,
            expiry_height=expiry_height,
            nullifier_hex=nullifier_hex,
            recipient_hex=recipient_hex,
            token_hex=token_hex,
            withdrawal_id=withdrawal_id,
            withdrawal_leaf_digest_hex=withdrawal_leaf_digest_hex,
            withdrawal_leaf_hex=withdrawal_leaf_hex,
        )


        bridge_withdrawal_response.additional_properties = d
        return bridge_withdrawal_response

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
