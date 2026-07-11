from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.block_response_by_market import BlockResponseByMarket
  from ..models.block_response_clearing_prices_nanos import BlockResponseClearingPricesNanos
  from ..models.bridge_block_response import BridgeBlockResponse
  from ..models.fill_response import FillResponse
  from ..models.rejection_response import RejectionResponse
  from ..models.system_event_response_type_0 import SystemEventResponseType0
  from ..models.system_event_response_type_1 import SystemEventResponseType1
  from ..models.system_event_response_type_2 import SystemEventResponseType2
  from ..models.system_event_response_type_3 import SystemEventResponseType3
  from ..models.system_event_response_type_4 import SystemEventResponseType4
  from ..models.system_event_response_type_5 import SystemEventResponseType5





T = TypeVar("T", bound="BlockResponse")



@_attrs_define
class BlockResponse:
    """ 
        Attributes:
            events_root (str):
            fill_count (int):
            height (int):
            order_count (int):
            orders_filled (int):
            parent_hash (str):
            state_root (str):
            timestamp_ms (int):
            total_volume_nanos (int): Total traded notional in the block. Integer nanodollars;
                1_000_000_000 = $1.
            total_welfare_nanos (int): Total solver welfare in the block. Integer nanodollars;
                1_000_000_000 = $1. Signed: solver rounding can yield small negatives.
            bridge (BridgeBlockResponse | Unset):
            by_market (BlockResponseByMarket | Unset): Nested per-market block scalars. Each
                `BlockMarketStats` carries the per-market splits for this block. Old
                clients ignore it; new clients consume what they recognise.
            clearing_prices_nanos (BlockResponseClearingPricesNanos | Unset): Clearing price vectors by market/group.
                Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
            fills (list[FillResponse] | Unset):
            rejections (list[RejectionResponse] | Unset):
            system_events (list[SystemEventResponseType0 | SystemEventResponseType1 | SystemEventResponseType2 |
                SystemEventResponseType3 | SystemEventResponseType4 | SystemEventResponseType5] | Unset):
            unique_placers (int | Unset): Unique placers (non-MM accounts) admitted into this block. Platform
                scalar — `by_market[m].placers` is the per-market split.
     """

    events_root: str
    fill_count: int
    height: int
    order_count: int
    orders_filled: int
    parent_hash: str
    state_root: str
    timestamp_ms: int
    total_volume_nanos: int
    total_welfare_nanos: int
    bridge: BridgeBlockResponse | Unset = UNSET
    by_market: BlockResponseByMarket | Unset = UNSET
    clearing_prices_nanos: BlockResponseClearingPricesNanos | Unset = UNSET
    fills: list[FillResponse] | Unset = UNSET
    rejections: list[RejectionResponse] | Unset = UNSET
    system_events: list[SystemEventResponseType0 | SystemEventResponseType1 | SystemEventResponseType2 | SystemEventResponseType3 | SystemEventResponseType4 | SystemEventResponseType5] | Unset = UNSET
    unique_placers: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.block_response_by_market import BlockResponseByMarket
        from ..models.block_response_clearing_prices_nanos import BlockResponseClearingPricesNanos
        from ..models.bridge_block_response import BridgeBlockResponse
        from ..models.fill_response import FillResponse
        from ..models.rejection_response import RejectionResponse
        from ..models.system_event_response_type_0 import SystemEventResponseType0
        from ..models.system_event_response_type_1 import SystemEventResponseType1
        from ..models.system_event_response_type_2 import SystemEventResponseType2
        from ..models.system_event_response_type_3 import SystemEventResponseType3
        from ..models.system_event_response_type_4 import SystemEventResponseType4
        from ..models.system_event_response_type_5 import SystemEventResponseType5
        events_root = self.events_root

        fill_count = self.fill_count

        height = self.height

        order_count = self.order_count

        orders_filled = self.orders_filled

        parent_hash = self.parent_hash

        state_root = self.state_root

        timestamp_ms = self.timestamp_ms

        total_volume_nanos = self.total_volume_nanos

        total_welfare_nanos = self.total_welfare_nanos

        bridge: dict[str, Any] | Unset = UNSET
        if not isinstance(self.bridge, Unset):
            bridge = self.bridge.to_dict()

        by_market: dict[str, Any] | Unset = UNSET
        if not isinstance(self.by_market, Unset):
            by_market = self.by_market.to_dict()

        clearing_prices_nanos: dict[str, Any] | Unset = UNSET
        if not isinstance(self.clearing_prices_nanos, Unset):
            clearing_prices_nanos = self.clearing_prices_nanos.to_dict()

        fills: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.fills, Unset):
            fills = []
            for fills_item_data in self.fills:
                fills_item = fills_item_data.to_dict()
                fills.append(fills_item)



        rejections: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.rejections, Unset):
            rejections = []
            for rejections_item_data in self.rejections:
                rejections_item = rejections_item_data.to_dict()
                rejections.append(rejections_item)



        system_events: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.system_events, Unset):
            system_events = []
            for system_events_item_data in self.system_events:
                system_events_item: dict[str, Any]
                if isinstance(system_events_item_data, SystemEventResponseType0):
                    system_events_item = system_events_item_data.to_dict()
                elif isinstance(system_events_item_data, SystemEventResponseType1):
                    system_events_item = system_events_item_data.to_dict()
                elif isinstance(system_events_item_data, SystemEventResponseType2):
                    system_events_item = system_events_item_data.to_dict()
                elif isinstance(system_events_item_data, SystemEventResponseType3):
                    system_events_item = system_events_item_data.to_dict()
                elif isinstance(system_events_item_data, SystemEventResponseType4):
                    system_events_item = system_events_item_data.to_dict()
                else:
                    system_events_item = system_events_item_data.to_dict()

                system_events.append(system_events_item)



        unique_placers = self.unique_placers


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "events_root": events_root,
            "fill_count": fill_count,
            "height": height,
            "order_count": order_count,
            "orders_filled": orders_filled,
            "parent_hash": parent_hash,
            "state_root": state_root,
            "timestamp_ms": timestamp_ms,
            "total_volume_nanos": total_volume_nanos,
            "total_welfare_nanos": total_welfare_nanos,
        })
        if bridge is not UNSET:
            field_dict["bridge"] = bridge
        if by_market is not UNSET:
            field_dict["by_market"] = by_market
        if clearing_prices_nanos is not UNSET:
            field_dict["clearing_prices_nanos"] = clearing_prices_nanos
        if fills is not UNSET:
            field_dict["fills"] = fills
        if rejections is not UNSET:
            field_dict["rejections"] = rejections
        if system_events is not UNSET:
            field_dict["system_events"] = system_events
        if unique_placers is not UNSET:
            field_dict["unique_placers"] = unique_placers

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.block_response_by_market import BlockResponseByMarket
        from ..models.block_response_clearing_prices_nanos import BlockResponseClearingPricesNanos
        from ..models.bridge_block_response import BridgeBlockResponse
        from ..models.fill_response import FillResponse
        from ..models.rejection_response import RejectionResponse
        from ..models.system_event_response_type_0 import SystemEventResponseType0
        from ..models.system_event_response_type_1 import SystemEventResponseType1
        from ..models.system_event_response_type_2 import SystemEventResponseType2
        from ..models.system_event_response_type_3 import SystemEventResponseType3
        from ..models.system_event_response_type_4 import SystemEventResponseType4
        from ..models.system_event_response_type_5 import SystemEventResponseType5
        d = dict(src_dict)
        events_root = d.pop("events_root")

        fill_count = d.pop("fill_count")

        height = d.pop("height")

        order_count = d.pop("order_count")

        orders_filled = d.pop("orders_filled")

        parent_hash = d.pop("parent_hash")

        state_root = d.pop("state_root")

        timestamp_ms = d.pop("timestamp_ms")

        total_volume_nanos = d.pop("total_volume_nanos")

        total_welfare_nanos = d.pop("total_welfare_nanos")

        _bridge = d.pop("bridge", UNSET)
        bridge: BridgeBlockResponse | Unset
        if isinstance(_bridge,  Unset):
            bridge = UNSET
        else:
            bridge = BridgeBlockResponse.from_dict(_bridge)




        _by_market = d.pop("by_market", UNSET)
        by_market: BlockResponseByMarket | Unset
        if isinstance(_by_market,  Unset):
            by_market = UNSET
        else:
            by_market = BlockResponseByMarket.from_dict(_by_market)




        _clearing_prices_nanos = d.pop("clearing_prices_nanos", UNSET)
        clearing_prices_nanos: BlockResponseClearingPricesNanos | Unset
        if isinstance(_clearing_prices_nanos,  Unset):
            clearing_prices_nanos = UNSET
        else:
            clearing_prices_nanos = BlockResponseClearingPricesNanos.from_dict(_clearing_prices_nanos)




        _fills = d.pop("fills", UNSET)
        fills: list[FillResponse] | Unset = UNSET
        if _fills is not UNSET:
            fills = []
            for fills_item_data in _fills:
                fills_item = FillResponse.from_dict(fills_item_data)



                fills.append(fills_item)


        _rejections = d.pop("rejections", UNSET)
        rejections: list[RejectionResponse] | Unset = UNSET
        if _rejections is not UNSET:
            rejections = []
            for rejections_item_data in _rejections:
                rejections_item = RejectionResponse.from_dict(rejections_item_data)



                rejections.append(rejections_item)


        _system_events = d.pop("system_events", UNSET)
        system_events: list[SystemEventResponseType0 | SystemEventResponseType1 | SystemEventResponseType2 | SystemEventResponseType3 | SystemEventResponseType4 | SystemEventResponseType5] | Unset = UNSET
        if _system_events is not UNSET:
            system_events = []
            for system_events_item_data in _system_events:
                def _parse_system_events_item(data: object) -> SystemEventResponseType0 | SystemEventResponseType1 | SystemEventResponseType2 | SystemEventResponseType3 | SystemEventResponseType4 | SystemEventResponseType5:
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_system_event_response_type_0 = SystemEventResponseType0.from_dict(data)



                        return componentsschemas_system_event_response_type_0
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_system_event_response_type_1 = SystemEventResponseType1.from_dict(data)



                        return componentsschemas_system_event_response_type_1
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_system_event_response_type_2 = SystemEventResponseType2.from_dict(data)



                        return componentsschemas_system_event_response_type_2
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_system_event_response_type_3 = SystemEventResponseType3.from_dict(data)



                        return componentsschemas_system_event_response_type_3
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_system_event_response_type_4 = SystemEventResponseType4.from_dict(data)



                        return componentsschemas_system_event_response_type_4
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_system_event_response_type_5 = SystemEventResponseType5.from_dict(data)



                    return componentsschemas_system_event_response_type_5

                system_events_item = _parse_system_events_item(system_events_item_data)

                system_events.append(system_events_item)


        unique_placers = d.pop("unique_placers", UNSET)

        block_response = cls(
            events_root=events_root,
            fill_count=fill_count,
            height=height,
            order_count=order_count,
            orders_filled=orders_filled,
            parent_hash=parent_hash,
            state_root=state_root,
            timestamp_ms=timestamp_ms,
            total_volume_nanos=total_volume_nanos,
            total_welfare_nanos=total_welfare_nanos,
            bridge=bridge,
            by_market=by_market,
            clearing_prices_nanos=clearing_prices_nanos,
            fills=fills,
            rejections=rejections,
            system_events=system_events,
            unique_placers=unique_placers,
        )


        block_response.additional_properties = d
        return block_response

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
