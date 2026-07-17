from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.time_in_force import TimeInForce
from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.order_spec_type_0 import OrderSpecType0
  from ..models.order_spec_type_1 import OrderSpecType1
  from ..models.order_spec_type_2 import OrderSpecType2
  from ..models.order_spec_type_3 import OrderSpecType3





T = TypeVar("T", bound="SubmitOrderRequest")



@_attrs_define
class SubmitOrderRequest:
    """ 
        Attributes:
            account_id (int): Account ID submitting the orders.
            orders (list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3]): Orders to submit.
            expires_at_block (int | None | Unset): Last eligible block height for explicit-expiry orders.
            mm_budget_nanos (None | str | Unset): If set, treat these orders as market maker orders with flash liquidity.
                The value is the MM's total capital budget. Integer nanodollars;
                1_000_000_000 = $1.
                MM orders skip per-order balance validation; instead the solver enforces
                the portfolio-level budget constraint at clearing time.
            time_in_force (TimeInForce | Unset):
     """

    account_id: int
    orders: list[OrderSpecType0 | OrderSpecType1 | OrderSpecType2 | OrderSpecType3]
    expires_at_block: int | None | Unset = UNSET
    mm_budget_nanos: None | str | Unset = UNSET
    time_in_force: TimeInForce | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        account_id = self.account_id

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



        expires_at_block: int | None | Unset
        if isinstance(self.expires_at_block, Unset):
            expires_at_block = UNSET
        else:
            expires_at_block = self.expires_at_block

        mm_budget_nanos: None | str | Unset
        if isinstance(self.mm_budget_nanos, Unset):
            mm_budget_nanos = UNSET
        else:
            mm_budget_nanos = self.mm_budget_nanos

        time_in_force: str | Unset = UNSET
        if not isinstance(self.time_in_force, Unset):
            time_in_force = self.time_in_force.value



        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "orders": orders,
        })
        if expires_at_block is not UNSET:
            field_dict["expires_at_block"] = expires_at_block
        if mm_budget_nanos is not UNSET:
            field_dict["mm_budget_nanos"] = mm_budget_nanos
        if time_in_force is not UNSET:
            field_dict["time_in_force"] = time_in_force

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.order_spec_type_0 import OrderSpecType0
        from ..models.order_spec_type_1 import OrderSpecType1
        from ..models.order_spec_type_2 import OrderSpecType2
        from ..models.order_spec_type_3 import OrderSpecType3
        d = dict(src_dict)
        account_id = d.pop("account_id")

        orders = []
        _orders = d.pop("orders")
        for orders_item_data in (_orders):
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


        def _parse_expires_at_block(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expires_at_block = _parse_expires_at_block(d.pop("expires_at_block", UNSET))


        def _parse_mm_budget_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mm_budget_nanos = _parse_mm_budget_nanos(d.pop("mm_budget_nanos", UNSET))


        _time_in_force = d.pop("time_in_force", UNSET)
        time_in_force: TimeInForce | Unset
        if isinstance(_time_in_force,  Unset):
            time_in_force = UNSET
        else:
            time_in_force = TimeInForce(_time_in_force)




        submit_order_request = cls(
            account_id=account_id,
            orders=orders,
            expires_at_block=expires_at_block,
            mm_budget_nanos=mm_budget_nanos,
            time_in_force=time_in_force,
        )


        submit_order_request.additional_properties = d
        return submit_order_request

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
