from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MarketResponse")



@_attrs_define
class MarketResponse:
    """ 
        Attributes:
            market_id (int):
            name (str):
            status (str):
            categories (list[str] | None | Unset): All category buckets the parent event matched on the mirror's
                tag-to-bucket lookup (e.g. `["Sports", "Politics"]`). Frontend picks
                one for display via its own priority list. None for sybil-native
                markets (use the singular `category` field instead).
            category (None | str | Unset):
            closed (bool | None | Unset): Whether Polymarket has closed this market. Off-block; the frontend
                filters closed markets out of the listing.
            created_at_ms (int | None | Unset):
            description (None | str | Unset):
            event_end_date_ms (int | None | Unset): Event-level expected end date (epoch ms). Display only.
            event_icon_url (None | str | Unset): Event-level icon URL (secondary image fallback).
            event_id (None | str | Unset): Polymarket parent event id — frontend grouping key.
            event_image_url (None | str | Unset): Event-level image URL.
            event_start_date_ms (int | None | Unset): Parent event start date (epoch ms) from Polymarket. Display/sort only.
            event_title (None | str | Unset): Polymarket parent event title.
            expiry_timestamp_ms (int | None | Unset):
            external_url (None | str | Unset): External URL (e.g., Polymarket link).
            group_item_title (None | str | Unset): Polymarket short outcome label (`groupItemTitle`, e.g. "May 15"). Off-
                block; the frontend uses it as the per-outcome name so it needn't fetch
                the raw event JSON just for labels.
            liquidity_avg10_nanos (int | Unset): Rolling last-10-batch band depth average. Integer nanodollars;
                1_000_000_000 = $1. Zero for markets without a clearing price yet.
                Pair with `liquidity_band_nanos` for labelling.
            liquidity_band_nanos (int | Unset): Width of the band the liquidity score uses (the ± in "$X ±$0.05").
                Integer nanodollars; 1_000_000_000 = $1.
                Always the live config value — `0` when no liquidity has been
                recorded yet.
            market_end_date_ms (int | None | Unset): Per-market expected end date (epoch ms). Display only.
            market_icon_url (None | str | Unset): Per-market icon URL (secondary image fallback).
            market_image_url (None | str | Unset): Per-market image URL.
            market_start_date_ms (int | None | Unset): Per-market start date (epoch ms) from Polymarket. Display/sort only.
            no_price_24h_ago_nanos (int | None | Unset): Clearing NO price ~24h ago. See `yes_price_24h_ago_nanos`.
                Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            no_price_nanos (int | None | Unset): Current NO clearing price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            orders_matched_total (int | Unset): All-time admissions that received at least one fill (B5's
                `has_been_matched` true at removal time). Cancels are NOT counted.
            orders_placed_total (int | Unset): All-time non-MM admissions counted against this market. Multi-market
                orders credit every active market; sum-of-per-market over-counts vs.
                the platform total — that's the documented attribution rule.
            orders_unmatched_total (int | Unset): All-time admissions that exited the book without any fill. Cancels
                are tracked separately and do not count here.
            payout_nanos (int | None | Unset): Resolution payout per YES share. Integer nanodollars; 1_000_000_000 = $1.
                Payouts are per-share probabilities in [0, 1e9].
            polymarket_condition_id (None | str | Unset): Polymarket on-chain condition id — FE join key into
                `GET /v1/events/{event_id}/raw` `markets[].conditionId`. Off-block.
            reference_price_expires_at_ms (int | None | Unset): Server-side expiry of `reference_price_nanos`, as Unix
                milliseconds.
                The price is omitted once this boundary has passed.
            reference_price_nanos (int | None | Unset): Reference price from external system (e.g., Polymarket), display
                only.
                Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            resolution_criteria (None | str | Unset):
            tags (list[str] | None | Unset):
            trader_count (int | Unset): All-time unique trader count for this market (decision Q-table:
                MM, MINT, multi-market split, etc.). Off-block — "since last
                restart" until prod persistence is enabled.
            volume_24h_nanos (int | Unset): Rolling 24h trading volume. Integer nanodollars; 1_000_000_000 = $1.
                Off-block;
                "since last restart" until prod persistence is enabled.
            volume_nanos (int | Unset): All-time traded notional. Integer nanodollars; 1_000_000_000 = $1.
            yes_price_24h_ago_nanos (int | None | Unset): Clearing YES price ~24h ago, derived from the per-market
                hourly snapshot. `None` for markets younger than 24h or wiped on
                restart. FE computes the 24h delta as `current - snapshot`.
                Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            yes_price_nanos (int | None | Unset): Current YES clearing price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
     """

    market_id: int
    name: str
    status: str
    categories: list[str] | None | Unset = UNSET
    category: None | str | Unset = UNSET
    closed: bool | None | Unset = UNSET
    created_at_ms: int | None | Unset = UNSET
    description: None | str | Unset = UNSET
    event_end_date_ms: int | None | Unset = UNSET
    event_icon_url: None | str | Unset = UNSET
    event_id: None | str | Unset = UNSET
    event_image_url: None | str | Unset = UNSET
    event_start_date_ms: int | None | Unset = UNSET
    event_title: None | str | Unset = UNSET
    expiry_timestamp_ms: int | None | Unset = UNSET
    external_url: None | str | Unset = UNSET
    group_item_title: None | str | Unset = UNSET
    liquidity_avg10_nanos: int | Unset = UNSET
    liquidity_band_nanos: int | Unset = UNSET
    market_end_date_ms: int | None | Unset = UNSET
    market_icon_url: None | str | Unset = UNSET
    market_image_url: None | str | Unset = UNSET
    market_start_date_ms: int | None | Unset = UNSET
    no_price_24h_ago_nanos: int | None | Unset = UNSET
    no_price_nanos: int | None | Unset = UNSET
    orders_matched_total: int | Unset = UNSET
    orders_placed_total: int | Unset = UNSET
    orders_unmatched_total: int | Unset = UNSET
    payout_nanos: int | None | Unset = UNSET
    polymarket_condition_id: None | str | Unset = UNSET
    reference_price_expires_at_ms: int | None | Unset = UNSET
    reference_price_nanos: int | None | Unset = UNSET
    resolution_criteria: None | str | Unset = UNSET
    tags: list[str] | None | Unset = UNSET
    trader_count: int | Unset = UNSET
    volume_24h_nanos: int | Unset = UNSET
    volume_nanos: int | Unset = UNSET
    yes_price_24h_ago_nanos: int | None | Unset = UNSET
    yes_price_nanos: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        name = self.name

        status = self.status

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

        created_at_ms: int | None | Unset
        if isinstance(self.created_at_ms, Unset):
            created_at_ms = UNSET
        else:
            created_at_ms = self.created_at_ms

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

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

        expiry_timestamp_ms: int | None | Unset
        if isinstance(self.expiry_timestamp_ms, Unset):
            expiry_timestamp_ms = UNSET
        else:
            expiry_timestamp_ms = self.expiry_timestamp_ms

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

        liquidity_avg10_nanos = self.liquidity_avg10_nanos

        liquidity_band_nanos = self.liquidity_band_nanos

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

        no_price_24h_ago_nanos: int | None | Unset
        if isinstance(self.no_price_24h_ago_nanos, Unset):
            no_price_24h_ago_nanos = UNSET
        else:
            no_price_24h_ago_nanos = self.no_price_24h_ago_nanos

        no_price_nanos: int | None | Unset
        if isinstance(self.no_price_nanos, Unset):
            no_price_nanos = UNSET
        else:
            no_price_nanos = self.no_price_nanos

        orders_matched_total = self.orders_matched_total

        orders_placed_total = self.orders_placed_total

        orders_unmatched_total = self.orders_unmatched_total

        payout_nanos: int | None | Unset
        if isinstance(self.payout_nanos, Unset):
            payout_nanos = UNSET
        else:
            payout_nanos = self.payout_nanos

        polymarket_condition_id: None | str | Unset
        if isinstance(self.polymarket_condition_id, Unset):
            polymarket_condition_id = UNSET
        else:
            polymarket_condition_id = self.polymarket_condition_id

        reference_price_expires_at_ms: int | None | Unset
        if isinstance(self.reference_price_expires_at_ms, Unset):
            reference_price_expires_at_ms = UNSET
        else:
            reference_price_expires_at_ms = self.reference_price_expires_at_ms

        reference_price_nanos: int | None | Unset
        if isinstance(self.reference_price_nanos, Unset):
            reference_price_nanos = UNSET
        else:
            reference_price_nanos = self.reference_price_nanos

        resolution_criteria: None | str | Unset
        if isinstance(self.resolution_criteria, Unset):
            resolution_criteria = UNSET
        else:
            resolution_criteria = self.resolution_criteria

        tags: list[str] | None | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, list):
            tags = self.tags


        else:
            tags = self.tags

        trader_count = self.trader_count

        volume_24h_nanos = self.volume_24h_nanos

        volume_nanos = self.volume_nanos

        yes_price_24h_ago_nanos: int | None | Unset
        if isinstance(self.yes_price_24h_ago_nanos, Unset):
            yes_price_24h_ago_nanos = UNSET
        else:
            yes_price_24h_ago_nanos = self.yes_price_24h_ago_nanos

        yes_price_nanos: int | None | Unset
        if isinstance(self.yes_price_nanos, Unset):
            yes_price_nanos = UNSET
        else:
            yes_price_nanos = self.yes_price_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "name": name,
            "status": status,
        })
        if categories is not UNSET:
            field_dict["categories"] = categories
        if category is not UNSET:
            field_dict["category"] = category
        if closed is not UNSET:
            field_dict["closed"] = closed
        if created_at_ms is not UNSET:
            field_dict["created_at_ms"] = created_at_ms
        if description is not UNSET:
            field_dict["description"] = description
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
        if expiry_timestamp_ms is not UNSET:
            field_dict["expiry_timestamp_ms"] = expiry_timestamp_ms
        if external_url is not UNSET:
            field_dict["external_url"] = external_url
        if group_item_title is not UNSET:
            field_dict["group_item_title"] = group_item_title
        if liquidity_avg10_nanos is not UNSET:
            field_dict["liquidity_avg10_nanos"] = liquidity_avg10_nanos
        if liquidity_band_nanos is not UNSET:
            field_dict["liquidity_band_nanos"] = liquidity_band_nanos
        if market_end_date_ms is not UNSET:
            field_dict["market_end_date_ms"] = market_end_date_ms
        if market_icon_url is not UNSET:
            field_dict["market_icon_url"] = market_icon_url
        if market_image_url is not UNSET:
            field_dict["market_image_url"] = market_image_url
        if market_start_date_ms is not UNSET:
            field_dict["market_start_date_ms"] = market_start_date_ms
        if no_price_24h_ago_nanos is not UNSET:
            field_dict["no_price_24h_ago_nanos"] = no_price_24h_ago_nanos
        if no_price_nanos is not UNSET:
            field_dict["no_price_nanos"] = no_price_nanos
        if orders_matched_total is not UNSET:
            field_dict["orders_matched_total"] = orders_matched_total
        if orders_placed_total is not UNSET:
            field_dict["orders_placed_total"] = orders_placed_total
        if orders_unmatched_total is not UNSET:
            field_dict["orders_unmatched_total"] = orders_unmatched_total
        if payout_nanos is not UNSET:
            field_dict["payout_nanos"] = payout_nanos
        if polymarket_condition_id is not UNSET:
            field_dict["polymarket_condition_id"] = polymarket_condition_id
        if reference_price_expires_at_ms is not UNSET:
            field_dict["reference_price_expires_at_ms"] = reference_price_expires_at_ms
        if reference_price_nanos is not UNSET:
            field_dict["reference_price_nanos"] = reference_price_nanos
        if resolution_criteria is not UNSET:
            field_dict["resolution_criteria"] = resolution_criteria
        if tags is not UNSET:
            field_dict["tags"] = tags
        if trader_count is not UNSET:
            field_dict["trader_count"] = trader_count
        if volume_24h_nanos is not UNSET:
            field_dict["volume_24h_nanos"] = volume_24h_nanos
        if volume_nanos is not UNSET:
            field_dict["volume_nanos"] = volume_nanos
        if yes_price_24h_ago_nanos is not UNSET:
            field_dict["yes_price_24h_ago_nanos"] = yes_price_24h_ago_nanos
        if yes_price_nanos is not UNSET:
            field_dict["yes_price_nanos"] = yes_price_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        name = d.pop("name")

        status = d.pop("status")

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


        def _parse_created_at_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        created_at_ms = _parse_created_at_ms(d.pop("created_at_ms", UNSET))


        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))


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


        def _parse_expiry_timestamp_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expiry_timestamp_ms = _parse_expiry_timestamp_ms(d.pop("expiry_timestamp_ms", UNSET))


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


        liquidity_avg10_nanos = d.pop("liquidity_avg10_nanos", UNSET)

        liquidity_band_nanos = d.pop("liquidity_band_nanos", UNSET)

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


        def _parse_no_price_24h_ago_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        no_price_24h_ago_nanos = _parse_no_price_24h_ago_nanos(d.pop("no_price_24h_ago_nanos", UNSET))


        def _parse_no_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        no_price_nanos = _parse_no_price_nanos(d.pop("no_price_nanos", UNSET))


        orders_matched_total = d.pop("orders_matched_total", UNSET)

        orders_placed_total = d.pop("orders_placed_total", UNSET)

        orders_unmatched_total = d.pop("orders_unmatched_total", UNSET)

        def _parse_payout_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        payout_nanos = _parse_payout_nanos(d.pop("payout_nanos", UNSET))


        def _parse_polymarket_condition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        polymarket_condition_id = _parse_polymarket_condition_id(d.pop("polymarket_condition_id", UNSET))


        def _parse_reference_price_expires_at_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        reference_price_expires_at_ms = _parse_reference_price_expires_at_ms(d.pop("reference_price_expires_at_ms", UNSET))


        def _parse_reference_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        reference_price_nanos = _parse_reference_price_nanos(d.pop("reference_price_nanos", UNSET))


        def _parse_resolution_criteria(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        resolution_criteria = _parse_resolution_criteria(d.pop("resolution_criteria", UNSET))


        def _parse_tags(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                tags_type_0 = cast(list[str], data)

                return tags_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        tags = _parse_tags(d.pop("tags", UNSET))


        trader_count = d.pop("trader_count", UNSET)

        volume_24h_nanos = d.pop("volume_24h_nanos", UNSET)

        volume_nanos = d.pop("volume_nanos", UNSET)

        def _parse_yes_price_24h_ago_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_price_24h_ago_nanos = _parse_yes_price_24h_ago_nanos(d.pop("yes_price_24h_ago_nanos", UNSET))


        def _parse_yes_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_price_nanos = _parse_yes_price_nanos(d.pop("yes_price_nanos", UNSET))


        market_response = cls(
            market_id=market_id,
            name=name,
            status=status,
            categories=categories,
            category=category,
            closed=closed,
            created_at_ms=created_at_ms,
            description=description,
            event_end_date_ms=event_end_date_ms,
            event_icon_url=event_icon_url,
            event_id=event_id,
            event_image_url=event_image_url,
            event_start_date_ms=event_start_date_ms,
            event_title=event_title,
            expiry_timestamp_ms=expiry_timestamp_ms,
            external_url=external_url,
            group_item_title=group_item_title,
            liquidity_avg10_nanos=liquidity_avg10_nanos,
            liquidity_band_nanos=liquidity_band_nanos,
            market_end_date_ms=market_end_date_ms,
            market_icon_url=market_icon_url,
            market_image_url=market_image_url,
            market_start_date_ms=market_start_date_ms,
            no_price_24h_ago_nanos=no_price_24h_ago_nanos,
            no_price_nanos=no_price_nanos,
            orders_matched_total=orders_matched_total,
            orders_placed_total=orders_placed_total,
            orders_unmatched_total=orders_unmatched_total,
            payout_nanos=payout_nanos,
            polymarket_condition_id=polymarket_condition_id,
            reference_price_expires_at_ms=reference_price_expires_at_ms,
            reference_price_nanos=reference_price_nanos,
            resolution_criteria=resolution_criteria,
            tags=tags,
            trader_count=trader_count,
            volume_24h_nanos=volume_24h_nanos,
            volume_nanos=volume_nanos,
            yes_price_24h_ago_nanos=yes_price_24h_ago_nanos,
            yes_price_nanos=yes_price_nanos,
        )


        market_response.additional_properties = d
        return market_response

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
