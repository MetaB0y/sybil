from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.submit_l1_withdrawal_event_request_status import SubmitL1WithdrawalEventRequestStatus
from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="SubmitL1WithdrawalEventRequest")



@_attrs_define
class SubmitL1WithdrawalEventRequest:
    """ 
        Attributes:
            event_at_unix (int): Event timestamp from the vault event, in Unix seconds.
            l1_block_height (int): Confirmed L1 block number carrying the event.
            nullifier_hex (str): Withdrawal nullifier emitted by SybilVault.
            status (SubmitL1WithdrawalEventRequestStatus): Queue state observed from the vault event.
            executable_at_unix (int | None | Unset): Finalization ETA emitted by the vault, in Unix seconds.
            tx_hash_hex (None | str | Unset): L1 transaction hash carrying the event, if indexed from logs.
     """

    event_at_unix: int
    l1_block_height: int
    nullifier_hex: str
    status: SubmitL1WithdrawalEventRequestStatus
    executable_at_unix: int | None | Unset = UNSET
    tx_hash_hex: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        event_at_unix = self.event_at_unix

        l1_block_height = self.l1_block_height

        nullifier_hex = self.nullifier_hex

        status = self.status.value

        executable_at_unix: int | None | Unset
        if isinstance(self.executable_at_unix, Unset):
            executable_at_unix = UNSET
        else:
            executable_at_unix = self.executable_at_unix

        tx_hash_hex: None | str | Unset
        if isinstance(self.tx_hash_hex, Unset):
            tx_hash_hex = UNSET
        else:
            tx_hash_hex = self.tx_hash_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "event_at_unix": event_at_unix,
            "l1_block_height": l1_block_height,
            "nullifier_hex": nullifier_hex,
            "status": status,
        })
        if executable_at_unix is not UNSET:
            field_dict["executable_at_unix"] = executable_at_unix
        if tx_hash_hex is not UNSET:
            field_dict["tx_hash_hex"] = tx_hash_hex

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        event_at_unix = d.pop("event_at_unix")

        l1_block_height = d.pop("l1_block_height")

        nullifier_hex = d.pop("nullifier_hex")

        status = SubmitL1WithdrawalEventRequestStatus(d.pop("status"))




        def _parse_executable_at_unix(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        executable_at_unix = _parse_executable_at_unix(d.pop("executable_at_unix", UNSET))


        def _parse_tx_hash_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tx_hash_hex = _parse_tx_hash_hex(d.pop("tx_hash_hex", UNSET))


        submit_l1_withdrawal_event_request = cls(
            event_at_unix=event_at_unix,
            l1_block_height=l1_block_height,
            nullifier_hex=nullifier_hex,
            status=status,
            executable_at_unix=executable_at_unix,
            tx_hash_hex=tx_hash_hex,
        )


        submit_l1_withdrawal_event_request.additional_properties = d
        return submit_l1_withdrawal_event_request

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
