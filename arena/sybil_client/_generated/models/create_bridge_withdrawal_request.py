from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="CreateBridgeWithdrawalRequest")



@_attrs_define
class CreateBridgeWithdrawalRequest:
    """ 
        Attributes:
            account_id (int): Sybil account whose available balance is burned.
            amount_token_units (int): Token base units released by the vault.
            chain_id (int): Destination chain id.
            recipient_hex (str): Hex-encoded L1 recipient address (20 bytes).
            token_address_hex (str): Hex-encoded token contract address (20 bytes).
            vault_address_hex (str): Hex-encoded vault contract address (20 bytes).
            expiry_height (int | None | Unset): Last Sybil block height at which this withdrawal leaf is valid.
            nonce (int | None | Unset): Per-account replay nonce. Required for signed bridge withdrawals.
     """

    account_id: int
    amount_token_units: int
    chain_id: int
    recipient_hex: str
    token_address_hex: str
    vault_address_hex: str
    expiry_height: int | None | Unset = UNSET
    nonce: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        amount_token_units = self.amount_token_units

        chain_id = self.chain_id

        recipient_hex = self.recipient_hex

        token_address_hex = self.token_address_hex

        vault_address_hex = self.vault_address_hex

        expiry_height: int | None | Unset
        if isinstance(self.expiry_height, Unset):
            expiry_height = UNSET
        else:
            expiry_height = self.expiry_height

        nonce: int | None | Unset
        if isinstance(self.nonce, Unset):
            nonce = UNSET
        else:
            nonce = self.nonce


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "amount_token_units": amount_token_units,
            "chain_id": chain_id,
            "recipient_hex": recipient_hex,
            "token_address_hex": token_address_hex,
            "vault_address_hex": vault_address_hex,
        })
        if expiry_height is not UNSET:
            field_dict["expiry_height"] = expiry_height
        if nonce is not UNSET:
            field_dict["nonce"] = nonce

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        amount_token_units = d.pop("amount_token_units")

        chain_id = d.pop("chain_id")

        recipient_hex = d.pop("recipient_hex")

        token_address_hex = d.pop("token_address_hex")

        vault_address_hex = d.pop("vault_address_hex")

        def _parse_expiry_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expiry_height = _parse_expiry_height(d.pop("expiry_height", UNSET))


        def _parse_nonce(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        nonce = _parse_nonce(d.pop("nonce", UNSET))


        create_bridge_withdrawal_request = cls(
            account_id=account_id,
            amount_token_units=amount_token_units,
            chain_id=chain_id,
            recipient_hex=recipient_hex,
            token_address_hex=token_address_hex,
            vault_address_hex=vault_address_hex,
            expiry_height=expiry_height,
            nonce=nonce,
        )


        create_bridge_withdrawal_request.additional_properties = d
        return create_bridge_withdrawal_request

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
