from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MarketSearchParams")



@_attrs_define
class MarketSearchParams:
    """ Query parameters for market search.

        Attributes:
            category (None | str | Unset): Exact category match.
            limit (int | None | Unset):
            max_yes_price_nanos (None | str | Unset): Maximum YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            min_volume_nanos (None | str | Unset): Minimum cumulative traded notional. Integer nanodollars; 1_000_000_000 =
                $1.
            min_yes_price_nanos (None | str | Unset): Minimum YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            offset (int | None | Unset):
            q (None | str | Unset): Text search (searches name + description).
            sort (None | str | Unset): Sort field: "volume", "created_at", "name", "price".
            status (None | str | Unset): Status filter ("active" or "resolved").
            tags (None | str | Unset): Comma-separated tags to filter by.
     """

    category: None | str | Unset = UNSET
    limit: int | None | Unset = UNSET
    max_yes_price_nanos: None | str | Unset = UNSET
    min_volume_nanos: None | str | Unset = UNSET
    min_yes_price_nanos: None | str | Unset = UNSET
    offset: int | None | Unset = UNSET
    q: None | str | Unset = UNSET
    sort: None | str | Unset = UNSET
    status: None | str | Unset = UNSET
    tags: None | str | Unset = UNSET





    def to_dict(self) -> dict[str, Any]:
        category: None | str | Unset
        if isinstance(self.category, Unset):
            category = UNSET
        else:
            category = self.category

        limit: int | None | Unset
        if isinstance(self.limit, Unset):
            limit = UNSET
        else:
            limit = self.limit

        max_yes_price_nanos: None | str | Unset
        if isinstance(self.max_yes_price_nanos, Unset):
            max_yes_price_nanos = UNSET
        else:
            max_yes_price_nanos = self.max_yes_price_nanos

        min_volume_nanos: None | str | Unset
        if isinstance(self.min_volume_nanos, Unset):
            min_volume_nanos = UNSET
        else:
            min_volume_nanos = self.min_volume_nanos

        min_yes_price_nanos: None | str | Unset
        if isinstance(self.min_yes_price_nanos, Unset):
            min_yes_price_nanos = UNSET
        else:
            min_yes_price_nanos = self.min_yes_price_nanos

        offset: int | None | Unset
        if isinstance(self.offset, Unset):
            offset = UNSET
        else:
            offset = self.offset

        q: None | str | Unset
        if isinstance(self.q, Unset):
            q = UNSET
        else:
            q = self.q

        sort: None | str | Unset
        if isinstance(self.sort, Unset):
            sort = UNSET
        else:
            sort = self.sort

        status: None | str | Unset
        if isinstance(self.status, Unset):
            status = UNSET
        else:
            status = self.status

        tags: None | str | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        else:
            tags = self.tags


        field_dict: dict[str, Any] = {}

        field_dict.update({
        })
        if category is not UNSET:
            field_dict["category"] = category
        if limit is not UNSET:
            field_dict["limit"] = limit
        if max_yes_price_nanos is not UNSET:
            field_dict["max_yes_price_nanos"] = max_yes_price_nanos
        if min_volume_nanos is not UNSET:
            field_dict["min_volume_nanos"] = min_volume_nanos
        if min_yes_price_nanos is not UNSET:
            field_dict["min_yes_price_nanos"] = min_yes_price_nanos
        if offset is not UNSET:
            field_dict["offset"] = offset
        if q is not UNSET:
            field_dict["q"] = q
        if sort is not UNSET:
            field_dict["sort"] = sort
        if status is not UNSET:
            field_dict["status"] = status
        if tags is not UNSET:
            field_dict["tags"] = tags

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        def _parse_category(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        category = _parse_category(d.pop("category", UNSET))


        def _parse_limit(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        limit = _parse_limit(d.pop("limit", UNSET))


        def _parse_max_yes_price_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        max_yes_price_nanos = _parse_max_yes_price_nanos(d.pop("max_yes_price_nanos", UNSET))


        def _parse_min_volume_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        min_volume_nanos = _parse_min_volume_nanos(d.pop("min_volume_nanos", UNSET))


        def _parse_min_yes_price_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        min_yes_price_nanos = _parse_min_yes_price_nanos(d.pop("min_yes_price_nanos", UNSET))


        def _parse_offset(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        offset = _parse_offset(d.pop("offset", UNSET))


        def _parse_q(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        q = _parse_q(d.pop("q", UNSET))


        def _parse_sort(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        sort = _parse_sort(d.pop("sort", UNSET))


        def _parse_status(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        status = _parse_status(d.pop("status", UNSET))


        def _parse_tags(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tags = _parse_tags(d.pop("tags", UNSET))


        market_search_params = cls(
            category=category,
            limit=limit,
            max_yes_price_nanos=max_yes_price_nanos,
            min_volume_nanos=min_volume_nanos,
            min_yes_price_nanos=min_yes_price_nanos,
            offset=offset,
            q=q,
            sort=sort,
            status=status,
            tags=tags,
        )

        return market_search_params

