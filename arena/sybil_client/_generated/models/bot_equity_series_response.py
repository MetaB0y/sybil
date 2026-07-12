from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.bot_equity_point_response import BotEquityPointResponse





T = TypeVar("T", bound="BotEquitySeriesResponse")



@_attrs_define
class BotEquitySeriesResponse:
    """ 
        Attributes:
            db_available (bool):
            downsampled (bool):
            limit (int):
            points (list[BotEquityPointResponse]):
            returned_rows (int):
            server_cap (int):
            source_rows (int):
            stride (int):
            db_path (None | str | Unset):
            error (None | str | Unset):
            since (None | str | Unset):
            trader (None | str | Unset):
     """

    db_available: bool
    downsampled: bool
    limit: int
    points: list[BotEquityPointResponse]
    returned_rows: int
    server_cap: int
    source_rows: int
    stride: int
    db_path: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    since: None | str | Unset = UNSET
    trader: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.bot_equity_point_response import BotEquityPointResponse
        db_available = self.db_available

        downsampled = self.downsampled

        limit = self.limit

        points = []
        for points_item_data in self.points:
            points_item = points_item_data.to_dict()
            points.append(points_item)



        returned_rows = self.returned_rows

        server_cap = self.server_cap

        source_rows = self.source_rows

        stride = self.stride

        db_path: None | str | Unset
        if isinstance(self.db_path, Unset):
            db_path = UNSET
        else:
            db_path = self.db_path

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        since: None | str | Unset
        if isinstance(self.since, Unset):
            since = UNSET
        else:
            since = self.since

        trader: None | str | Unset
        if isinstance(self.trader, Unset):
            trader = UNSET
        else:
            trader = self.trader


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "db_available": db_available,
            "downsampled": downsampled,
            "limit": limit,
            "points": points,
            "returned_rows": returned_rows,
            "server_cap": server_cap,
            "source_rows": source_rows,
            "stride": stride,
        })
        if db_path is not UNSET:
            field_dict["db_path"] = db_path
        if error is not UNSET:
            field_dict["error"] = error
        if since is not UNSET:
            field_dict["since"] = since
        if trader is not UNSET:
            field_dict["trader"] = trader

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bot_equity_point_response import BotEquityPointResponse
        d = dict(src_dict)
        db_available = d.pop("db_available")

        downsampled = d.pop("downsampled")

        limit = d.pop("limit")

        points = []
        _points = d.pop("points")
        for points_item_data in (_points):
            points_item = BotEquityPointResponse.from_dict(points_item_data)



            points.append(points_item)


        returned_rows = d.pop("returned_rows")

        server_cap = d.pop("server_cap")

        source_rows = d.pop("source_rows")

        stride = d.pop("stride")

        def _parse_db_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        db_path = _parse_db_path(d.pop("db_path", UNSET))


        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))


        def _parse_since(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        since = _parse_since(d.pop("since", UNSET))


        def _parse_trader(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        trader = _parse_trader(d.pop("trader", UNSET))


        bot_equity_series_response = cls(
            db_available=db_available,
            downsampled=downsampled,
            limit=limit,
            points=points,
            returned_rows=returned_rows,
            server_cap=server_cap,
            source_rows=source_rows,
            stride=stride,
            db_path=db_path,
            error=error,
            since=since,
            trader=trader,
        )


        bot_equity_series_response.additional_properties = d
        return bot_equity_series_response

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
