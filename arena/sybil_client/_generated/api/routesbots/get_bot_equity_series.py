from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.bot_equity_series_response import BotEquitySeriesResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    *,
    trader: str | Unset = UNSET,
    since: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["trader"] = trader

    params["since"] = since

    params["limit"] = limit


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/bots/equity-series",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> BotEquitySeriesResponse | None:
    if response.status_code == 200:
        response_200 = BotEquitySeriesResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[BotEquitySeriesResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    trader: str | Unset = UNSET,
    since: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[BotEquitySeriesResponse]:
    """ GET /v1/bots/equity-series

     Public per-bot portfolio-value series proxied from Arena's private typed
    read service. Dense results are bounded and downsampled by Arena.

    Args:
        trader (str | Unset):
        since (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BotEquitySeriesResponse]
     """


    kwargs = _get_kwargs(
        trader=trader,
since=since,
limit=limit,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    trader: str | Unset = UNSET,
    since: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> BotEquitySeriesResponse | None:
    """ GET /v1/bots/equity-series

     Public per-bot portfolio-value series proxied from Arena's private typed
    read service. Dense results are bounded and downsampled by Arena.

    Args:
        trader (str | Unset):
        since (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BotEquitySeriesResponse
     """


    return sync_detailed(
        client=client,
trader=trader,
since=since,
limit=limit,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    trader: str | Unset = UNSET,
    since: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[BotEquitySeriesResponse]:
    """ GET /v1/bots/equity-series

     Public per-bot portfolio-value series proxied from Arena's private typed
    read service. Dense results are bounded and downsampled by Arena.

    Args:
        trader (str | Unset):
        since (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BotEquitySeriesResponse]
     """


    kwargs = _get_kwargs(
        trader=trader,
since=since,
limit=limit,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    trader: str | Unset = UNSET,
    since: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> BotEquitySeriesResponse | None:
    """ GET /v1/bots/equity-series

     Public per-bot portfolio-value series proxied from Arena's private typed
    read service. Dense results are bounded and downsampled by Arena.

    Args:
        trader (str | Unset):
        since (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BotEquitySeriesResponse
     """


    return (await asyncio_detailed(
        client=client,
trader=trader,
since=since,
limit=limit,

    )).parsed
