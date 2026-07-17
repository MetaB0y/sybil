from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="OpenBatchResponse")



@_attrs_define
class OpenBatchResponse:
    """ Response shape for `GET /v1/markets/{id}/open-batch`. B1 populates
    `unique_placers`; indicative fields stub `None`/`0` until C2.

        Attributes:
            unique_placers (int):
            indicative_computed_at_ms (int | Unset):
            indicative_no_price_nanos (None | str | Unset): Indicative NO price for the open batch. Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
            indicative_volume_nanos (str | Unset): Indicative traded notional for the open batch. Integer nanodollars;
                1_000_000_000 = $1.
            indicative_yes_price_nanos (None | str | Unset): Indicative YES price for the open batch. Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
     """

    unique_placers: int
    indicative_computed_at_ms: int | Unset = UNSET
    indicative_no_price_nanos: None | str | Unset = UNSET
    indicative_volume_nanos: str | Unset = UNSET
    indicative_yes_price_nanos: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        unique_placers = self.unique_placers

        indicative_computed_at_ms = self.indicative_computed_at_ms

        indicative_no_price_nanos: None | str | Unset
        if isinstance(self.indicative_no_price_nanos, Unset):
            indicative_no_price_nanos = UNSET
        else:
            indicative_no_price_nanos = self.indicative_no_price_nanos

        indicative_volume_nanos = self.indicative_volume_nanos

        indicative_yes_price_nanos: None | str | Unset
        if isinstance(self.indicative_yes_price_nanos, Unset):
            indicative_yes_price_nanos = UNSET
        else:
            indicative_yes_price_nanos = self.indicative_yes_price_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "unique_placers": unique_placers,
        })
        if indicative_computed_at_ms is not UNSET:
            field_dict["indicative_computed_at_ms"] = indicative_computed_at_ms
        if indicative_no_price_nanos is not UNSET:
            field_dict["indicative_no_price_nanos"] = indicative_no_price_nanos
        if indicative_volume_nanos is not UNSET:
            field_dict["indicative_volume_nanos"] = indicative_volume_nanos
        if indicative_yes_price_nanos is not UNSET:
            field_dict["indicative_yes_price_nanos"] = indicative_yes_price_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        unique_placers = d.pop("unique_placers")

        indicative_computed_at_ms = d.pop("indicative_computed_at_ms", UNSET)

        def _parse_indicative_no_price_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        indicative_no_price_nanos = _parse_indicative_no_price_nanos(d.pop("indicative_no_price_nanos", UNSET))


        indicative_volume_nanos = d.pop("indicative_volume_nanos", UNSET)

        def _parse_indicative_yes_price_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        indicative_yes_price_nanos = _parse_indicative_yes_price_nanos(d.pop("indicative_yes_price_nanos", UNSET))


        open_batch_response = cls(
            unique_placers=unique_placers,
            indicative_computed_at_ms=indicative_computed_at_ms,
            indicative_no_price_nanos=indicative_no_price_nanos,
            indicative_volume_nanos=indicative_volume_nanos,
            indicative_yes_price_nanos=indicative_yes_price_nanos,
        )


        open_batch_response.additional_properties = d
        return open_batch_response

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
