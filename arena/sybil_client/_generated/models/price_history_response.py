from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.price_point_response import PricePointResponse





T = TypeVar("T", bound="PriceHistoryResponse")



@_attrs_define
class PriceHistoryResponse:
    """ 
        Attributes:
            market_id (int):
            points (list[PricePointResponse]):
            history_complete_from_height (int | None | Unset): First source height represented after projection
                bootstrap/retention.
            indexed_through_height (int | None | Unset): Highest source block durably projected by the private history
                service.
            next_before_height (int | None | Unset):
            retention_min_height (int | None | Unset):
     """

    market_id: int
    points: list[PricePointResponse]
    history_complete_from_height: int | None | Unset = UNSET
    indexed_through_height: int | None | Unset = UNSET
    next_before_height: int | None | Unset = UNSET
    retention_min_height: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.price_point_response import PricePointResponse
        market_id = self.market_id

        points = []
        for points_item_data in self.points:
            points_item = points_item_data.to_dict()
            points.append(points_item)



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

        next_before_height: int | None | Unset
        if isinstance(self.next_before_height, Unset):
            next_before_height = UNSET
        else:
            next_before_height = self.next_before_height

        retention_min_height: int | None | Unset
        if isinstance(self.retention_min_height, Unset):
            retention_min_height = UNSET
        else:
            retention_min_height = self.retention_min_height


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "points": points,
        })
        if history_complete_from_height is not UNSET:
            field_dict["history_complete_from_height"] = history_complete_from_height
        if indexed_through_height is not UNSET:
            field_dict["indexed_through_height"] = indexed_through_height
        if next_before_height is not UNSET:
            field_dict["next_before_height"] = next_before_height
        if retention_min_height is not UNSET:
            field_dict["retention_min_height"] = retention_min_height

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.price_point_response import PricePointResponse
        d = dict(src_dict)
        market_id = d.pop("market_id")

        points = []
        _points = d.pop("points")
        for points_item_data in (_points):
            points_item = PricePointResponse.from_dict(points_item_data)



            points.append(points_item)


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


        def _parse_next_before_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        next_before_height = _parse_next_before_height(d.pop("next_before_height", UNSET))


        def _parse_retention_min_height(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        retention_min_height = _parse_retention_min_height(d.pop("retention_min_height", UNSET))


        price_history_response = cls(
            market_id=market_id,
            points=points,
            history_complete_from_height=history_complete_from_height,
            indexed_through_height=indexed_through_height,
            next_before_height=next_before_height,
            retention_min_height=retention_min_height,
        )


        price_history_response.additional_properties = d
        return price_history_response

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
