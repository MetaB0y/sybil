from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="SubmitL1DepositRequest")



@_attrs_define
class SubmitL1DepositRequest:
    """ 
        Attributes:
            amount_token_units (int): Token base units accepted by the vault, e.g. USDC's 6-decimal units.
            chain_id (int): Source chain id.
            deposit_id (int): Sequential L1 vault deposit id.
            deposit_root_hex (str): Hex-encoded post-deposit L1 deposit tree root (32 bytes).
            sender_hex (str): Hex-encoded L1 sender address (20 bytes).
            token_address_hex (str): Hex-encoded token contract address (20 bytes).
            vault_address_hex (str): Hex-encoded vault contract address (20 bytes).
            account_id (int | None | Unset): Sybil account receiving the credit. Must be absent when `quarantine` is true.
            quarantine (bool | Unset): Dispose an unresolvable raw key into the committed system quarantine ledger.
            sybil_account_key_hex (None | str | Unset): Optional Sybil bridge account key. If omitted, the API derives it
                for the account.
     """

    amount_token_units: int
    chain_id: int
    deposit_id: int
    deposit_root_hex: str
    sender_hex: str
    token_address_hex: str
    vault_address_hex: str
    account_id: int | None | Unset = UNSET
    quarantine: bool | Unset = UNSET
    sybil_account_key_hex: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        amount_token_units = self.amount_token_units

        chain_id = self.chain_id

        deposit_id = self.deposit_id

        deposit_root_hex = self.deposit_root_hex

        sender_hex = self.sender_hex

        token_address_hex = self.token_address_hex

        vault_address_hex = self.vault_address_hex

        account_id: int | None | Unset
        if isinstance(self.account_id, Unset):
            account_id = UNSET
        else:
            account_id = self.account_id

        quarantine = self.quarantine

        sybil_account_key_hex: None | str | Unset
        if isinstance(self.sybil_account_key_hex, Unset):
            sybil_account_key_hex = UNSET
        else:
            sybil_account_key_hex = self.sybil_account_key_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "amount_token_units": amount_token_units,
            "chain_id": chain_id,
            "deposit_id": deposit_id,
            "deposit_root_hex": deposit_root_hex,
            "sender_hex": sender_hex,
            "token_address_hex": token_address_hex,
            "vault_address_hex": vault_address_hex,
        })
        if account_id is not UNSET:
            field_dict["account_id"] = account_id
        if quarantine is not UNSET:
            field_dict["quarantine"] = quarantine
        if sybil_account_key_hex is not UNSET:
            field_dict["sybil_account_key_hex"] = sybil_account_key_hex

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        amount_token_units = d.pop("amount_token_units")

        chain_id = d.pop("chain_id")

        deposit_id = d.pop("deposit_id")

        deposit_root_hex = d.pop("deposit_root_hex")

        sender_hex = d.pop("sender_hex")

        token_address_hex = d.pop("token_address_hex")

        vault_address_hex = d.pop("vault_address_hex")

        def _parse_account_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        account_id = _parse_account_id(d.pop("account_id", UNSET))


        quarantine = d.pop("quarantine", UNSET)

        def _parse_sybil_account_key_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        sybil_account_key_hex = _parse_sybil_account_key_hex(d.pop("sybil_account_key_hex", UNSET))


        submit_l1_deposit_request = cls(
            amount_token_units=amount_token_units,
            chain_id=chain_id,
            deposit_id=deposit_id,
            deposit_root_hex=deposit_root_hex,
            sender_hex=sender_hex,
            token_address_hex=token_address_hex,
            vault_address_hex=vault_address_hex,
            account_id=account_id,
            quarantine=quarantine,
            sybil_account_key_hex=sybil_account_key_hex,
        )


        submit_l1_deposit_request.additional_properties = d
        return submit_l1_deposit_request

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
