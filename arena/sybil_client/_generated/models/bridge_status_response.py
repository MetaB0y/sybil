from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.bridge_domain_response import BridgeDomainResponse





T = TypeVar("T", bound="BridgeStatusResponse")



@_attrs_define
class BridgeStatusResponse:
    """ 
        Attributes:
            deposit_cursor (int):
            deposit_root_hex (str):
            next_withdrawal_id (int):
            observed_l1_height (int):
            withdrawal_count (int):
            cancelled_withdrawal_count (int | Unset):
            configured_domain (BridgeDomainResponse | None | Unset):
            finalized_withdrawal_count (int | Unset):
            quarantine_ledger_size (int | Unset):
            queued_withdrawal_count (int | Unset):
            refunded_withdrawal_count (int | Unset):
            total_quarantined_nanos (int | Unset): Sum of parked value. Integer nanodollars; 1_000_000_000 = $1.
     """

    deposit_cursor: int
    deposit_root_hex: str
    next_withdrawal_id: int
    observed_l1_height: int
    withdrawal_count: int
    cancelled_withdrawal_count: int | Unset = UNSET
    configured_domain: BridgeDomainResponse | None | Unset = UNSET
    finalized_withdrawal_count: int | Unset = UNSET
    quarantine_ledger_size: int | Unset = UNSET
    queued_withdrawal_count: int | Unset = UNSET
    refunded_withdrawal_count: int | Unset = UNSET
    total_quarantined_nanos: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.bridge_domain_response import BridgeDomainResponse
        deposit_cursor = self.deposit_cursor

        deposit_root_hex = self.deposit_root_hex

        next_withdrawal_id = self.next_withdrawal_id

        observed_l1_height = self.observed_l1_height

        withdrawal_count = self.withdrawal_count

        cancelled_withdrawal_count = self.cancelled_withdrawal_count

        configured_domain: dict[str, Any] | None | Unset
        if isinstance(self.configured_domain, Unset):
            configured_domain = UNSET
        elif isinstance(self.configured_domain, BridgeDomainResponse):
            configured_domain = self.configured_domain.to_dict()
        else:
            configured_domain = self.configured_domain

        finalized_withdrawal_count = self.finalized_withdrawal_count

        quarantine_ledger_size = self.quarantine_ledger_size

        queued_withdrawal_count = self.queued_withdrawal_count

        refunded_withdrawal_count = self.refunded_withdrawal_count

        total_quarantined_nanos = self.total_quarantined_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposit_cursor": deposit_cursor,
            "deposit_root_hex": deposit_root_hex,
            "next_withdrawal_id": next_withdrawal_id,
            "observed_l1_height": observed_l1_height,
            "withdrawal_count": withdrawal_count,
        })
        if cancelled_withdrawal_count is not UNSET:
            field_dict["cancelled_withdrawal_count"] = cancelled_withdrawal_count
        if configured_domain is not UNSET:
            field_dict["configured_domain"] = configured_domain
        if finalized_withdrawal_count is not UNSET:
            field_dict["finalized_withdrawal_count"] = finalized_withdrawal_count
        if quarantine_ledger_size is not UNSET:
            field_dict["quarantine_ledger_size"] = quarantine_ledger_size
        if queued_withdrawal_count is not UNSET:
            field_dict["queued_withdrawal_count"] = queued_withdrawal_count
        if refunded_withdrawal_count is not UNSET:
            field_dict["refunded_withdrawal_count"] = refunded_withdrawal_count
        if total_quarantined_nanos is not UNSET:
            field_dict["total_quarantined_nanos"] = total_quarantined_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bridge_domain_response import BridgeDomainResponse
        d = dict(src_dict)
        deposit_cursor = d.pop("deposit_cursor")

        deposit_root_hex = d.pop("deposit_root_hex")

        next_withdrawal_id = d.pop("next_withdrawal_id")

        observed_l1_height = d.pop("observed_l1_height")

        withdrawal_count = d.pop("withdrawal_count")

        cancelled_withdrawal_count = d.pop("cancelled_withdrawal_count", UNSET)

        def _parse_configured_domain(data: object) -> BridgeDomainResponse | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                configured_domain_type_1 = BridgeDomainResponse.from_dict(data)



                return configured_domain_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BridgeDomainResponse | None | Unset, data)

        configured_domain = _parse_configured_domain(d.pop("configured_domain", UNSET))


        finalized_withdrawal_count = d.pop("finalized_withdrawal_count", UNSET)

        quarantine_ledger_size = d.pop("quarantine_ledger_size", UNSET)

        queued_withdrawal_count = d.pop("queued_withdrawal_count", UNSET)

        refunded_withdrawal_count = d.pop("refunded_withdrawal_count", UNSET)

        total_quarantined_nanos = d.pop("total_quarantined_nanos", UNSET)

        bridge_status_response = cls(
            deposit_cursor=deposit_cursor,
            deposit_root_hex=deposit_root_hex,
            next_withdrawal_id=next_withdrawal_id,
            observed_l1_height=observed_l1_height,
            withdrawal_count=withdrawal_count,
            cancelled_withdrawal_count=cancelled_withdrawal_count,
            configured_domain=configured_domain,
            finalized_withdrawal_count=finalized_withdrawal_count,
            quarantine_ledger_size=quarantine_ledger_size,
            queued_withdrawal_count=queued_withdrawal_count,
            refunded_withdrawal_count=refunded_withdrawal_count,
            total_quarantined_nanos=total_quarantined_nanos,
        )


        bridge_status_response.additional_properties = d
        return bridge_status_response

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
