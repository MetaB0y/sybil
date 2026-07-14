from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.order_spec_type_0 import OrderSpecType0
  from ..models.order_spec_type_1 import OrderSpecType1
  from ..models.order_spec_type_2 import OrderSpecType2
  from ..models.order_spec_type_3 import OrderSpecType3





T = TypeVar("T", bound="ActorMarketIntent")



@_attrs_define
class ActorMarketIntent:
    """ 
        Attributes:
            market_id (int):
            orders (list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3] | Unset):
            skip_reason (None | str | Unset):
     """

    market_id: int
    orders: list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3] | Unset = UNSET
    skip_reason: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        market_id = self.market_id

        orders: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.orders, Unset):
            orders = []
            for orders_item_data in self.orders:
                orders_item: dict[str, Any]
                if isinstance(orders_item_data, OrderSpecType0):
                    orders_item = orders_item_data.to_dict()
                elif isinstance(orders_item_data, OrderSpecType1):
                    orders_item = orders_item_data.to_dict()
                elif isinstance(orders_item_data, OrderSpecType2):
                    orders_item = orders_item_data.to_dict()
                else:
                    orders_item = orders_item_data.to_dict()

                orders.append(orders_item)



        skip_reason: None | str | Unset
        if isinstance(self.skip_reason, Unset):
            skip_reason = UNSET
        else:
            skip_reason = self.skip_reason


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
        })
        if orders is not UNSET:
            field_dict["orders"] = orders
        if skip_reason is not UNSET:
            field_dict["skip_reason"] = skip_reason

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        d = dict(src_dict)
        market_id = d.pop("market_id")

        _orders = d.pop("orders", UNSET)
        orders: list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3] | Unset = UNSET
        if _orders is not UNSET:
            orders = []
            for orders_item_data in _orders:
                def _parse_orders_item(data: object) -> OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3:
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_order_spec_type_0 = OrderSpecType0.from_dict(data)



                        return componentsschemas_order_spec_type_0
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_order_spec_type_1 = OrderSpecType1.from_dict(data)



                        return componentsschemas_order_spec_type_1
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    try:
                        if not isinstance(data, dict):
                            raise TypeError()
                        componentsschemas_order_spec_type_2 = OrderSpecType2.from_dict(data)



                        return componentsschemas_order_spec_type_2
                    except (TypeError, ValueError, AttributeError, KeyError):
                        pass
                    if not isinstance(data, dict):
                        raise TypeError()
                    componentsschemas_order_spec_type_3 = OrderSpecType3.from_dict(data)



                    return componentsschemas_order_spec_type_3

                orders_item = _parse_orders_item(orders_item_data)

                orders.append(orders_item)


        def _parse_skip_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        skip_reason = _parse_skip_reason(d.pop("skip_reason", UNSET))


        actor_market_intent = cls(
            market_id=market_id,
            orders=orders,
            skip_reason=skip_reason,
        )


        actor_market_intent.additional_properties = d
        return actor_market_intent

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
