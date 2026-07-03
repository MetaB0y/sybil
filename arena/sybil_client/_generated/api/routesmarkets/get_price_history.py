from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.price_history_response import PriceHistoryResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    id: int,
    *,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_height: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["from_ms"] = from_ms

    params["to_ms"] = to_ms

    params["before_height"] = before_height

    params["limit"] = limit


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/markets/{id}/prices/history".format(id=quote(str(id), safe=""),),
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> PriceHistoryResponse | None:
    if response.status_code == 200:
        response_200 = PriceHistoryResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[PriceHistoryResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_height: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[PriceHistoryResponse]:
    """ GET /v1/markets/{id}/prices/history

    Args:
        id (int):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_height (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PriceHistoryResponse]
     """


    kwargs = _get_kwargs(
        id=id,
from_ms=from_ms,
to_ms=to_ms,
before_height=before_height,
limit=limit,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_height: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> PriceHistoryResponse | None:
    """ GET /v1/markets/{id}/prices/history

    Args:
        id (int):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_height (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PriceHistoryResponse
     """


    return sync_detailed(
        id=id,
client=client,
from_ms=from_ms,
to_ms=to_ms,
before_height=before_height,
limit=limit,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_height: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[PriceHistoryResponse]:
    """ GET /v1/markets/{id}/prices/history

    Args:
        id (int):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_height (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PriceHistoryResponse]
     """


    kwargs = _get_kwargs(
        id=id,
from_ms=from_ms,
to_ms=to_ms,
before_height=before_height,
limit=limit,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_height: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> PriceHistoryResponse | None:
    """ GET /v1/markets/{id}/prices/history

    Args:
        id (int):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_height (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PriceHistoryResponse
     """


    return (await asyncio_detailed(
        id=id,
client=client,
from_ms=from_ms,
to_ms=to_ms,
before_height=before_height,
limit=limit,

    )).parsed
