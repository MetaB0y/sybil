from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="SetMarketMetadataRequest")



@_attrs_define
class SetMarketMetadataRequest:
    """ 
        Attributes:
            categories (list[str] | None | Unset): All category buckets the parent event matched in the mirror's tag-to-
                bucket lookup (e.g. `["Sports", "Politics"]` for an NBA + Trump
                event). One per matched row; the frontend picks which to render
                using its own priority list, so reordering display priority is
                frontend-only.
            category (None | str | Unset): Single display category. **Legacy** — populated only for sybil-native
                markets at create time. Mirrored markets now use `categories` (plural)
                and let the frontend pick one for display via its own priority order.
            closed (bool | None | Unset): Whether Polymarket has closed this market. The frontend hides closed
                markets from the listing.
            event_end_date_ms (int | None | Unset): Event-level expected end date (epoch ms). Display only.
            event_icon_url (None | str | Unset): Event-level icon URL (secondary; frontend uses as `onError` fallback).
            event_id (None | str | Unset): Polymarket parent event id — used by the frontend to group sibling
                markets (e.g., "Fed Decision in June" sub-questions). Distinct from the
                matching engine's NegRisk `MarketGroup`, which it does not affect.
            event_image_url (None | str | Unset): Event-level image URL (primary).
            event_start_date_ms (int | None | Unset): Parent event start date (epoch ms). Display/sort only.
            event_title (None | str | Unset): Polymarket parent event title — rendered as the MultiCard header.
            external_url (None | str | Unset): External URL (e.g., Polymarket link).
            group_item_title (None | str | Unset): Polymarket short outcome label (`groupItemTitle`, e.g. "May 15"). The
                frontend renders this as the per-outcome name on multi-cards.
            market_end_date_ms (int | None | Unset): Per-market expected end date (epoch ms). Display only; matching engine
                does not enforce trading cutoffs at this time.
            market_icon_url (None | str | Unset): Per-market icon URL (secondary; frontend uses as `onError` fallback).
            market_image_url (None | str | Unset): Per-market image URL (primary).
            market_start_date_ms (int | None | Unset): Per-market start date (epoch ms). Display/sort only.
            polymarket_condition_id (None | str | Unset): Polymarket on-chain condition id — the FE join key into the event
                JSON
                snapshot (`/v1/events/{id}/raw` `markets[].conditionId`).
     """

    categories: list[str] | None | Unset = UNSET
    category: None | str | Unset = UNSET
    closed: bool | None | Unset = UNSET
    event_end_date_ms: int | None | Unset = UNSET
    event_icon_url: None | str | Unset = UNSET
    event_id: None | str | Unset = UNSET
    event_image_url: None | str | Unset = UNSET
    event_start_date_ms: int | None | Unset = UNSET
    event_title: None | str | Unset = UNSET
    external_url: None | str | Unset = UNSET
    group_item_title: None | str | Unset = UNSET
    market_end_date_ms: int | None | Unset = UNSET
    market_icon_url: None | str | Unset = UNSET
    market_image_url: None | str | Unset = UNSET
    market_start_date_ms: int | None | Unset = UNSET
    polymarket_condition_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        categories: list[str] | None | Unset
        if isinstance(self.categories, Unset):
            categories = UNSET
        elif isinstance(self.categories, list):
            categories = self.categories


        else:
            categories = self.categories

        category: None | str | Unset
        if isinstance(self.category, Unset):
            category = UNSET
        else:
            category = self.category

        closed: bool | None | Unset
        if isinstance(self.closed, Unset):
            closed = UNSET
        else:
            closed = self.closed

        event_end_date_ms: int | None | Unset
        if isinstance(self.event_end_date_ms, Unset):
            event_end_date_ms = UNSET
        else:
            event_end_date_ms = self.event_end_date_ms

        event_icon_url: None | str | Unset
        if isinstance(self.event_icon_url, Unset):
            event_icon_url = UNSET
        else:
            event_icon_url = self.event_icon_url

        event_id: None | str | Unset
        if isinstance(self.event_id, Unset):
            event_id = UNSET
        else:
            event_id = self.event_id

        event_image_url: None | str | Unset
        if isinstance(self.event_image_url, Unset):
            event_image_url = UNSET
        else:
            event_image_url = self.event_image_url

        event_start_date_ms: int | None | Unset
        if isinstance(self.event_start_date_ms, Unset):
            event_start_date_ms = UNSET
        else:
            event_start_date_ms = self.event_start_date_ms

        event_title: None | str | Unset
        if isinstance(self.event_title, Unset):
            event_title = UNSET
        else:
            event_title = self.event_title

        external_url: None | str | Unset
        if isinstance(self.external_url, Unset):
            external_url = UNSET
        else:
            external_url = self.external_url

        group_item_title: None | str | Unset
        if isinstance(self.group_item_title, Unset):
            group_item_title = UNSET
        else:
            group_item_title = self.group_item_title

        market_end_date_ms: int | None | Unset
        if isinstance(self.market_end_date_ms, Unset):
            market_end_date_ms = UNSET
        else:
            market_end_date_ms = self.market_end_date_ms

        market_icon_url: None | str | Unset
        if isinstance(self.market_icon_url, Unset):
            market_icon_url = UNSET
        else:
            market_icon_url = self.market_icon_url

        market_image_url: None | str | Unset
        if isinstance(self.market_image_url, Unset):
            market_image_url = UNSET
        else:
            market_image_url = self.market_image_url

        market_start_date_ms: int | None | Unset
        if isinstance(self.market_start_date_ms, Unset):
            market_start_date_ms = UNSET
        else:
            market_start_date_ms = self.market_start_date_ms

        polymarket_condition_id: None | str | Unset
        if isinstance(self.polymarket_condition_id, Unset):
            polymarket_condition_id = UNSET
        else:
            polymarket_condition_id = self.polymarket_condition_id


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if categories is not UNSET:
            field_dict["categories"] = categories
        if category is not UNSET:
            field_dict["category"] = category
        if closed is not UNSET:
            field_dict["closed"] = closed
        if event_end_date_ms is not UNSET:
            field_dict["event_end_date_ms"] = event_end_date_ms
        if event_icon_url is not UNSET:
            field_dict["event_icon_url"] = event_icon_url
        if event_id is not UNSET:
            field_dict["event_id"] = event_id
        if event_image_url is not UNSET:
            field_dict["event_image_url"] = event_image_url
        if event_start_date_ms is not UNSET:
            field_dict["event_start_date_ms"] = event_start_date_ms
        if event_title is not UNSET:
            field_dict["event_title"] = event_title
        if external_url is not UNSET:
            field_dict["external_url"] = external_url
        if group_item_title is not UNSET:
            field_dict["group_item_title"] = group_item_title
        if market_end_date_ms is not UNSET:
            field_dict["market_end_date_ms"] = market_end_date_ms
        if market_icon_url is not UNSET:
            field_dict["market_icon_url"] = market_icon_url
        if market_image_url is not UNSET:
            field_dict["market_image_url"] = market_image_url
        if market_start_date_ms is not UNSET:
            field_dict["market_start_date_ms"] = market_start_date_ms
        if polymarket_condition_id is not UNSET:
            field_dict["polymarket_condition_id"] = polymarket_condition_id

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        def _parse_categories(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                categories_type_0 = cast(list[str], data)

                return categories_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        categories = _parse_categories(d.pop("categories", UNSET))


        def _parse_category(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        category = _parse_category(d.pop("category", UNSET))


        def _parse_closed(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        closed = _parse_closed(d.pop("closed", UNSET))


        def _parse_event_end_date_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        event_end_date_ms = _parse_event_end_date_ms(d.pop("event_end_date_ms", UNSET))


        def _parse_event_icon_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        event_icon_url = _parse_event_icon_url(d.pop("event_icon_url", UNSET))


        def _parse_event_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        event_id = _parse_event_id(d.pop("event_id", UNSET))


        def _parse_event_image_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        event_image_url = _parse_event_image_url(d.pop("event_image_url", UNSET))


        def _parse_event_start_date_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        event_start_date_ms = _parse_event_start_date_ms(d.pop("event_start_date_ms", UNSET))


        def _parse_event_title(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        event_title = _parse_event_title(d.pop("event_title", UNSET))


        def _parse_external_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        external_url = _parse_external_url(d.pop("external_url", UNSET))


        def _parse_group_item_title(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        group_item_title = _parse_group_item_title(d.pop("group_item_title", UNSET))


        def _parse_market_end_date_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        market_end_date_ms = _parse_market_end_date_ms(d.pop("market_end_date_ms", UNSET))


        def _parse_market_icon_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        market_icon_url = _parse_market_icon_url(d.pop("market_icon_url", UNSET))


        def _parse_market_image_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        market_image_url = _parse_market_image_url(d.pop("market_image_url", UNSET))


        def _parse_market_start_date_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        market_start_date_ms = _parse_market_start_date_ms(d.pop("market_start_date_ms", UNSET))


        def _parse_polymarket_condition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        polymarket_condition_id = _parse_polymarket_condition_id(d.pop("polymarket_condition_id", UNSET))


        set_market_metadata_request = cls(
            categories=categories,
            category=category,
            closed=closed,
            event_end_date_ms=event_end_date_ms,
            event_icon_url=event_icon_url,
            event_id=event_id,
            event_image_url=event_image_url,
            event_start_date_ms=event_start_date_ms,
            event_title=event_title,
            external_url=external_url,
            group_item_title=group_item_title,
            market_end_date_ms=market_end_date_ms,
            market_icon_url=market_icon_url,
            market_image_url=market_image_url,
            market_start_date_ms=market_start_date_ms,
            polymarket_condition_id=polymarket_condition_id,
        )


        set_market_metadata_request.additional_properties = d
        return set_market_metadata_request

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
