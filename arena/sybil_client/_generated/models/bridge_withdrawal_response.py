from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.bridge_withdrawal_l1_status import BridgeWithdrawalL1Status
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BridgeWithdrawalResponse")



@_attrs_define
class BridgeWithdrawalResponse:
    """ 
        Attributes:
            account_id (int):
            amount_nanos (int): Off-chain balance amount burned for the withdrawal. Integer nanodollars;
                1_000_000_000 = $1.
            amount_token_units (int): Token base units released by the vault.
            created_at_height (int):
            expiry_height (int):
            nullifier_hex (str):
            recipient_hex (str):
            token_hex (str):
            withdrawal_id (int):
            withdrawal_leaf_digest_hex (str):
            withdrawal_leaf_hex (str):
            l1_cancelled_at_unix (int | None | Unset):
            l1_executable_at_unix (int | None | Unset):
            l1_finalized_at_unix (int | None | Unset):
            l1_requested_at_unix (int | None | Unset):
            l1_status (BridgeWithdrawalL1Status | Unset):
            l1_tx_hash_hex (None | str | Unset):
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
    l1_cancelled_at_unix: int | None | Unset = UNSET
    l1_executable_at_unix: int | None | Unset = UNSET
    l1_finalized_at_unix: int | None | Unset = UNSET
    l1_requested_at_unix: int | None | Unset = UNSET
    l1_status: BridgeWithdrawalL1Status | Unset = UNSET
    l1_tx_hash_hex: None | str | Unset = UNSET
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

        l1_cancelled_at_unix: int | None | Unset
        if isinstance(self.l1_cancelled_at_unix, Unset):
            l1_cancelled_at_unix = UNSET
        else:
            l1_cancelled_at_unix = self.l1_cancelled_at_unix

        l1_executable_at_unix: int | None | Unset
        if isinstance(self.l1_executable_at_unix, Unset):
            l1_executable_at_unix = UNSET
        else:
            l1_executable_at_unix = self.l1_executable_at_unix

        l1_finalized_at_unix: int | None | Unset
        if isinstance(self.l1_finalized_at_unix, Unset):
            l1_finalized_at_unix = UNSET
        else:
            l1_finalized_at_unix = self.l1_finalized_at_unix

        l1_requested_at_unix: int | None | Unset
        if isinstance(self.l1_requested_at_unix, Unset):
            l1_requested_at_unix = UNSET
        else:
            l1_requested_at_unix = self.l1_requested_at_unix

        l1_status: str | Unset = UNSET
        if not isinstance(self.l1_status, Unset):
            l1_status = self.l1_status.value


        l1_tx_hash_hex: None | str | Unset
        if isinstance(self.l1_tx_hash_hex, Unset):
            l1_tx_hash_hex = UNSET
        else:
            l1_tx_hash_hex = self.l1_tx_hash_hex


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
        if l1_cancelled_at_unix is not UNSET:
            field_dict["l1_cancelled_at_unix"] = l1_cancelled_at_unix
        if l1_executable_at_unix is not UNSET:
            field_dict["l1_executable_at_unix"] = l1_executable_at_unix
        if l1_finalized_at_unix is not UNSET:
            field_dict["l1_finalized_at_unix"] = l1_finalized_at_unix
        if l1_requested_at_unix is not UNSET:
            field_dict["l1_requested_at_unix"] = l1_requested_at_unix
        if l1_status is not UNSET:
            field_dict["l1_status"] = l1_status
        if l1_tx_hash_hex is not UNSET:
            field_dict["l1_tx_hash_hex"] = l1_tx_hash_hex

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

        def _parse_l1_cancelled_at_unix(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        l1_cancelled_at_unix = _parse_l1_cancelled_at_unix(d.pop("l1_cancelled_at_unix", UNSET))


        def _parse_l1_executable_at_unix(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        l1_executable_at_unix = _parse_l1_executable_at_unix(d.pop("l1_executable_at_unix", UNSET))


        def _parse_l1_finalized_at_unix(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        l1_finalized_at_unix = _parse_l1_finalized_at_unix(d.pop("l1_finalized_at_unix", UNSET))


        def _parse_l1_requested_at_unix(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        l1_requested_at_unix = _parse_l1_requested_at_unix(d.pop("l1_requested_at_unix", UNSET))


        _l1_status = d.pop("l1_status", UNSET)
        l1_status: BridgeWithdrawalL1Status | Unset
        if isinstance(_l1_status,  Unset):
            l1_status = UNSET
        else:
            l1_status = BridgeWithdrawalL1Status(_l1_status)




        def _parse_l1_tx_hash_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        l1_tx_hash_hex = _parse_l1_tx_hash_hex(d.pop("l1_tx_hash_hex", UNSET))


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
            l1_cancelled_at_unix=l1_cancelled_at_unix,
            l1_executable_at_unix=l1_executable_at_unix,
            l1_finalized_at_unix=l1_finalized_at_unix,
            l1_requested_at_unix=l1_requested_at_unix,
            l1_status=l1_status,
            l1_tx_hash_hex=l1_tx_hash_hex,
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
