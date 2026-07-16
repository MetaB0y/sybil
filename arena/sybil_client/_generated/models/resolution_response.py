from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ResolutionResponse")



@_attrs_define
class ResolutionResponse:
    """ Detailed view of a market's resolution state. Active markets return
    `payout_nanos = None`.

        Attributes:
            market_id (int):
            status (str):
            template (str):
            payout_nanos (int | None | Unset): Resolution payout per YES share. Integer nanodollars;
                1_000_000_000 = $1. Payouts are per-share probabilities in [0, 1e9].
            resolved_at_ms (int | None | Unset):
            resolved_by_feed_id (int | None | Unset):
            resolved_by_feed_name (None | str | Unset):
     """

    market_id: int
    status: str
    template: str
    payout_nanos: int | None | Unset = UNSET
    resolved_at_ms: int | None | Unset = UNSET
    resolved_by_feed_id: int | None | Unset = UNSET
    resolved_by_feed_name: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        status = self.status

        template = self.template

        payout_nanos: int | None | Unset
        if isinstance(self.payout_nanos, Unset):
            payout_nanos = UNSET
        else:
            payout_nanos = self.payout_nanos

        resolved_at_ms: int | None | Unset
        if isinstance(self.resolved_at_ms, Unset):
            resolved_at_ms = UNSET
        else:
            resolved_at_ms = self.resolved_at_ms

        resolved_by_feed_id: int | None | Unset
        if isinstance(self.resolved_by_feed_id, Unset):
            resolved_by_feed_id = UNSET
        else:
            resolved_by_feed_id = self.resolved_by_feed_id

        resolved_by_feed_name: None | str | Unset
        if isinstance(self.resolved_by_feed_name, Unset):
            resolved_by_feed_name = UNSET
        else:
            resolved_by_feed_name = self.resolved_by_feed_name


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "status": status,
            "template": template,
        })
        if payout_nanos is not UNSET:
            field_dict["payout_nanos"] = payout_nanos
        if resolved_at_ms is not UNSET:
            field_dict["resolved_at_ms"] = resolved_at_ms
        if resolved_by_feed_id is not UNSET:
            field_dict["resolved_by_feed_id"] = resolved_by_feed_id
        if resolved_by_feed_name is not UNSET:
            field_dict["resolved_by_feed_name"] = resolved_by_feed_name

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        status = d.pop("status")

        template = d.pop("template")

        def _parse_payout_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        payout_nanos = _parse_payout_nanos(d.pop("payout_nanos", UNSET))


        def _parse_resolved_at_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        resolved_at_ms = _parse_resolved_at_ms(d.pop("resolved_at_ms", UNSET))


        def _parse_resolved_by_feed_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        resolved_by_feed_id = _parse_resolved_by_feed_id(d.pop("resolved_by_feed_id", UNSET))


        def _parse_resolved_by_feed_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        resolved_by_feed_name = _parse_resolved_by_feed_name(d.pop("resolved_by_feed_name", UNSET))


        resolution_response = cls(
            market_id=market_id,
            status=status,
            template=template,
            payout_nanos=payout_nanos,
            resolved_at_ms=resolved_at_ms,
            resolved_by_feed_id=resolved_by_feed_id,
            resolved_by_feed_name=resolved_by_feed_name,
        )


        resolution_response.additional_properties = d
        return resolution_response

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
