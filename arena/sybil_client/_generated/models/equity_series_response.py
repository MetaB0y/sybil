from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.equity_point_response import EquityPointResponse





T = TypeVar("T", bound="EquitySeriesResponse")



@_attrs_define
class EquitySeriesResponse:
    """ 
        Attributes:
            account_id (int):
            downsampled (bool):
            history_scope (str): `durable` for redb-backed history, `memory` for bounded dev fallback.
            history_truncated (bool): True when the requested range begins before the retained boundary.
            points (list[EquityPointResponse]):
            source_points (int): Number of retained source samples represented by `points`.
            history_complete_from_height (int | None | Unset):
            indexed_through_height (int | None | Unset):
            retention_min_timestamp_ms (int | None | Unset): Oldest timestamp for which durable history is guaranteed
                complete.
                `None` means retention is disabled.
     """

    account_id: int
    downsampled: bool
    history_scope: str
    history_truncated: bool
    points: list[EquityPointResponse]
    source_points: int
    history_complete_from_height: int | None | Unset = UNSET
    indexed_through_height: int | None | Unset = UNSET
    retention_min_timestamp_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.equity_point_response import EquityPointResponse
        account_id = self.account_id

        downsampled = self.downsampled

        history_scope = self.history_scope

        history_truncated = self.history_truncated

        points = []
        for points_item_data in self.points:
            points_item = points_item_data.to_dict()
            points.append(points_item)



        source_points = self.source_points

        history_complete_from_height: int | None | Unset
        if isinstance(self.history_complete_from_height, Unset):
            history_complete_from_height = UNSET
        else:
            history_complete_from_height = self.history_complete_from_height

        indexed_through_height: int | None | Unset
        if isinstance(self.indexed_through_height, Unset):
            indexed_through_height = UNSET
        else:
            indexed_through_height = self.indexed_through_height

        retention_min_timestamp_ms: int | None | Unset
        if isinstance(self.retention_min_timestamp_ms, Unset):
            retention_min_timestamp_ms = UNSET
        else:
            retention_min_timestamp_ms = self.retention_min_timestamp_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "downsampled": downsampled,
            "history_scope": history_scope,
            "history_truncated": history_truncated,
            "points": points,
            "source_points": source_points,
        })
        if history_complete_from_height is not UNSET:
            field_dict["history_complete_from_height"] = history_complete_from_height
        if indexed_through_height is not UNSET:
            field_dict["indexed_through_height"] = indexed_through_height
        if retention_min_timestamp_ms is not UNSET:
            field_dict["retention_min_timestamp_ms"] = retention_min_timestamp_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.equity_point_response import EquityPointResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        downsampled = d.pop("downsampled")

        history_scope = d.pop("history_scope")

        history_truncated = d.pop("history_truncated")

        points = []
        _points = d.pop("points")
        for points_item_data in (_points):
            points_item = EquityPointResponse.from_dict(points_item_data)



            points.append(points_item)


        source_points = d.pop("source_points")

        def _parse_history_complete_from_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        history_complete_from_height = _parse_history_complete_from_height(d.pop("history_complete_from_height", UNSET))


        def _parse_indexed_through_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        indexed_through_height = _parse_indexed_through_height(d.pop("indexed_through_height", UNSET))


        def _parse_retention_min_timestamp_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        retention_min_timestamp_ms = _parse_retention_min_timestamp_ms(d.pop("retention_min_timestamp_ms", UNSET))


        equity_series_response = cls(
            account_id=account_id,
            downsampled=downsampled,
            history_scope=history_scope,
            history_truncated=history_truncated,
            points=points,
            source_points=source_points,
            history_complete_from_height=history_complete_from_height,
            indexed_through_height=indexed_through_height,
            retention_min_timestamp_ms=retention_min_timestamp_ms,
        )


        equity_series_response.additional_properties = d
        return equity_series_response

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
